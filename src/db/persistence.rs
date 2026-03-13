// src/db/persistence.rs
// SurrealDB Persistence Layer — replaces JSONL append-only log

use serde_json::Value;
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::{Database, Namespace, Root};
use surrealdb::Surreal;

use serde::Deserialize;

use crate::db::error::PersistenceError;
use crate::events::EventEnvelope;
use crate::nats::messages::{Presence, SessionNote, TaskAssignment, TaskStatusUpdate};
use crate::state::DatabaseAuthLevel;

/// SurrealQL schema — run once on init.
///
/// Tables are SCHEMALESS — we control all inputs (embedded in-memory DB) and
/// this avoids type coercion issues (chrono timestamps serialize as ISO strings,
/// not native SurrealDB datetime; empty vecs are omitted by serde).
/// Indexes on common query fields for efficient lookups.
const SCHEMA: &str = r#"
-- Presence heartbeats (ephemeral, 24h TTL concept — pruned by query)
DEFINE TABLE IF NOT EXISTS presence SCHEMALESS;
DEFINE INDEX IF NOT EXISTS agent_idx ON presence COLUMNS agent;
DEFINE INDEX IF NOT EXISTS timestamp_idx ON presence COLUMNS timestamp;

-- Task assignments
DEFINE TABLE IF NOT EXISTS task_assignment SCHEMALESS;
DEFINE INDEX IF NOT EXISTS task_id_idx ON task_assignment COLUMNS task_id;
DEFINE INDEX IF NOT EXISTS assigned_to_idx ON task_assignment COLUMNS assigned_to;

-- Task status updates
DEFINE TABLE IF NOT EXISTS task_status_update SCHEMALESS;
DEFINE INDEX IF NOT EXISTS task_id_idx ON task_status_update COLUMNS task_id;
DEFINE INDEX IF NOT EXISTS agent_idx ON task_status_update COLUMNS agent;

-- Session notes
DEFINE TABLE IF NOT EXISTS session_note SCHEMALESS;
DEFINE INDEX IF NOT EXISTS agent_idx ON session_note COLUMNS agent;
DEFINE INDEX IF NOT EXISTS project_idx ON session_note COLUMNS project;

-- EventEnvelope (local event log — replaces JSONL)
DEFINE TABLE IF NOT EXISTS event SCHEMALESS;
DEFINE INDEX IF NOT EXISTS event_id_idx ON event COLUMNS event_id UNIQUE;
DEFINE INDEX IF NOT EXISTS agent_idx ON event COLUMNS agent_id;
DEFINE INDEX IF NOT EXISTS type_idx ON event COLUMNS event_type;
DEFINE INDEX IF NOT EXISTS timestamp_idx ON event COLUMNS timestamp;

-- Agent read cursors (server-side tracking — runtime-agnostic)
DEFINE TABLE IF NOT EXISTS agent_read_cursor SCHEMALESS;
DEFINE INDEX IF NOT EXISTS agent_id_idx ON agent_read_cursor COLUMNS agent_id UNIQUE;
"#;

/// Read cursor state for an agent, as stored in SurrealDB.
#[derive(Debug, Clone, Deserialize)]
pub struct AgentReadCursor {
    pub agent_id: String,
    pub last_read_event_id: String,
    pub last_read_at: String,
}

/// SurrealDB persistence layer for ming-qiao.
///
/// Wraps a `Surreal<Any>` connection that routes by URL scheme at runtime:
/// - `mem://` — in-memory engine (tests, single-process default)
/// - `ws://host:port` — WebSocket to a shared SurrealDB server
///
/// Clone is cheap — `Surreal<Any>` is internally Arc'd.
#[derive(Clone)]
pub struct Persistence {
    db: Surreal<Any>,
}

impl Persistence {
    /// Convenience: create an in-memory SurrealDB (equivalent to `connect("mem://", None, None)`).
    ///
    /// Used by tests and `AppState::new()` (default config).
    pub async fn new() -> Result<Self, PersistenceError> {
        Self::connect("mem://", None, None).await
    }

    /// Connect to SurrealDB at the given URL.
    ///
    /// - `mem://` — in-memory engine (no auth needed)
    /// - `ws://host:port` — shared server (provide username/password)
    ///
    /// Supports three auth levels per Security P0:
    /// - `Root` — superuser access (legacy, avoid in production)
    /// - `Namespace` — scoped to astralmaris namespace
    /// - `Database` — scoped to astralmaris/mingqiao (recommended)
    ///
    /// Runs the schema with `IF NOT EXISTS` so multiple processes can connect
    /// to the same server without conflicting.
    pub async fn connect(
        url: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<Self, PersistenceError> {
        Self::connect_with_auth(url, username, password, DatabaseAuthLevel::Root).await
    }

    /// Connect with explicit auth level selection.
    pub async fn connect_with_auth(
        url: &str,
        username: Option<&str>,
        password: Option<&str>,
        auth_level: DatabaseAuthLevel,
    ) -> Result<Self, PersistenceError> {
        let db = surrealdb::engine::any::connect(url).await.map_err(db_err)?;

        // Authenticate if credentials provided (required for ws:// connections)
        if let (Some(user), Some(pass)) = (username, password) {
            match auth_level {
                DatabaseAuthLevel::Root => {
                    db.signin(Root {
                        username: user.to_string(),
                        password: pass.to_string(),
                    })
                    .await
                    .map_err(db_err)?;
                }
                DatabaseAuthLevel::Namespace => {
                    db.signin(Namespace {
                        namespace: "astralmaris".to_string(),
                        username: user.to_string(),
                        password: pass.to_string(),
                    })
                    .await
                    .map_err(db_err)?;
                }
                DatabaseAuthLevel::Database => {
                    db.signin(Database {
                        namespace: "astralmaris".to_string(),
                        database: "mingqiao".to_string(),
                        username: user.to_string(),
                        password: pass.to_string(),
                    })
                    .await
                    .map_err(db_err)?;
                }
            }
        }

        db.use_ns("astralmaris")
            .use_db("mingqiao")
            .await
            .map_err(db_err)?;
        db.query(SCHEMA).await.map_err(db_err)?;
        Ok(Self { db })
    }

    // =====================================================================
    // NATS message persistence (coordination bus)
    // =====================================================================

    /// Store a presence heartbeat.
    pub async fn store_presence(&self, p: &Presence) -> Result<(), PersistenceError> {
        let val = serde_json::to_value(p)?;
        self.db
            .query("CREATE presence CONTENT $data")
            .bind(("data", val))
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Store a task assignment (with denormalized project field).
    pub async fn store_task_assignment(
        &self,
        ta: &TaskAssignment,
        project: &str,
    ) -> Result<(), PersistenceError> {
        let mut val = serde_json::to_value(ta)?;
        if let Some(obj) = val.as_object_mut() {
            obj.insert("project".to_string(), Value::String(project.to_string()));
        }
        self.db
            .query("CREATE task_assignment CONTENT $data")
            .bind(("data", val))
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Store a task status update (with denormalized project field).
    pub async fn store_task_status(
        &self,
        ts: &TaskStatusUpdate,
        project: &str,
    ) -> Result<(), PersistenceError> {
        let mut val = serde_json::to_value(ts)?;
        if let Some(obj) = val.as_object_mut() {
            obj.insert("project".to_string(), Value::String(project.to_string()));
        }
        self.db
            .query("CREATE task_status_update CONTENT $data")
            .bind(("data", val))
            .await
            .map_err(db_err)?;
        Ok(())
    }

    /// Store session notes.
    pub async fn store_session_note(&self, sn: &SessionNote) -> Result<(), PersistenceError> {
        let val = serde_json::to_value(sn)?;
        self.db
            .query("CREATE session_note CONTENT $data")
            .bind(("data", val))
            .await
            .map_err(db_err)?;
        Ok(())
    }

    // =====================================================================
    // Local event persistence (replaces JSONL EventWriter/EventReader)
    // =====================================================================

    /// Store an event envelope. Returns the event ID string.
    pub async fn store_event(&self, event: &EventEnvelope) -> Result<String, PersistenceError> {
        let event_id = event.id.to_string();
        let event_type_str = event.event_type.to_string();

        // Build a storage-friendly value: rename `id` → `event_id`, flatten event_type
        let mut val = serde_json::to_value(event)?;
        if let Some(obj) = val.as_object_mut() {
            obj.remove("id");
            obj.insert("event_id".to_string(), Value::String(event_id.clone()));
            obj.insert("event_type".to_string(), Value::String(event_type_str));
        }

        self.db
            .query("CREATE event CONTENT $data")
            .bind(("data", val))
            .await
            .map_err(db_err)?;
        Ok(event_id)
    }

    /// Get a single event by its UUID string ID.
    pub async fn get_event(&self, id: &str) -> Result<Option<EventEnvelope>, PersistenceError> {
        let mut result = self.db
            .query("SELECT * OMIT id FROM event WHERE event_id = $id LIMIT 1")
            .bind(("id", id.to_string()))
            .await
            .map_err(db_err)?;
        let rows: Vec<Value> = result.take(0).map_err(db_err)?;
        rows.into_iter().next().map(row_to_envelope).transpose()
    }

    /// Get events after a given event ID (by timestamp), up to limit.
    pub async fn get_events_after(
        &self,
        after_id: &str,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, PersistenceError> {
        let mut result = self.db
            .query(
                "SELECT * OMIT id FROM event \
                 WHERE timestamp > (SELECT VALUE timestamp FROM event WHERE event_id = $after_id LIMIT 1) \
                 ORDER BY timestamp ASC \
                 LIMIT $limit"
            )
            .bind(("after_id", after_id.to_string()))
            .bind(("limit", limit))
            .await
            .map_err(db_err)?;
        let rows: Vec<Value> = result.take(0).map_err(db_err)?;
        rows.into_iter().map(row_to_envelope).collect()
    }

    /// Get events by type string (e.g. "message_sent"), up to limit.
    pub async fn get_events_by_type(
        &self,
        event_type: &str,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, PersistenceError> {
        let mut result = self.db
            .query(
                "SELECT * OMIT id FROM event WHERE event_type = $etype ORDER BY timestamp DESC LIMIT $limit",
            )
            .bind(("etype", event_type.to_string()))
            .bind(("limit", limit))
            .await
            .map_err(db_err)?;
        let rows: Vec<Value> = result.take(0).map_err(db_err)?;
        rows.into_iter().map(row_to_envelope).collect()
    }

    /// Get events produced by a specific agent, up to limit.
    pub async fn get_events_for_agent(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<EventEnvelope>, PersistenceError> {
        let mut result = self.db
            .query(
                "SELECT * OMIT id FROM event WHERE agent_id = $aid ORDER BY timestamp DESC LIMIT $limit",
            )
            .bind(("aid", agent_id.to_string()))
            .bind(("limit", limit))
            .await
            .map_err(db_err)?;
        let rows: Vec<Value> = result.take(0).map_err(db_err)?;
        rows.into_iter().map(row_to_envelope).collect()
    }

    /// Get all events, ordered by timestamp ascending.
    ///
    /// Used for Indexer hydration on startup — replays the full event log
    /// through `process_event()` to rebuild in-memory materialized views.
    pub async fn get_all_events(&self) -> Result<Vec<EventEnvelope>, PersistenceError> {
        let mut result = self.db
            .query("SELECT * OMIT id FROM event ORDER BY timestamp ASC")
            .await
            .map_err(db_err)?;
        let rows: Vec<Value> = result.take(0).map_err(db_err)?;
        rows.into_iter().map(row_to_envelope).collect()
    }

    /// Get events belonging to a thread, ordered by timestamp ascending.
    ///
    /// Queries the nested `payload.data.thread_id` field in SurrealDB.
    /// Used as a fallback when the Indexer hasn't synced cross-instance events yet.
    pub async fn get_events_by_thread_id(
        &self,
        thread_id: &str,
    ) -> Result<Vec<EventEnvelope>, PersistenceError> {
        let mut result = self.db
            .query(
                "SELECT * OMIT id FROM event \
                 WHERE payload.data.thread_id = $tid \
                 ORDER BY timestamp ASC",
            )
            .bind(("tid", thread_id.to_string()))
            .await
            .map_err(db_err)?;
        let rows: Vec<Value> = result.take(0).map_err(db_err)?;
        rows.into_iter().map(row_to_envelope).collect()
    }

    // =====================================================================
    // Agent read cursors (server-side tracking — runtime-agnostic)
    // =====================================================================

    /// Get the last-read event ID for an agent.
    ///
    /// Returns `None` if the agent has never read their inbox.
    pub async fn get_read_cursor(&self, agent_id: &str) -> Result<Option<String>, PersistenceError> {
        let mut result = self.db
            .query("SELECT VALUE last_read_event_id FROM agent_read_cursor WHERE agent_id = $aid LIMIT 1")
            .bind(("aid", agent_id.to_string()))
            .await
            .map_err(db_err)?;
        let rows: Vec<Option<String>> = result.take(0).map_err(db_err)?;
        Ok(rows.into_iter().next().flatten())
    }

    /// Get all read cursors across all agents.
    pub async fn get_all_cursors(&self) -> Result<Vec<AgentReadCursor>, PersistenceError> {
        let mut result = self.db
            .query("SELECT * OMIT id FROM agent_read_cursor ORDER BY agent_id ASC")
            .await
            .map_err(db_err)?;
        let rows: Vec<Value> = result.take(0).map_err(db_err)?;
        rows.into_iter()
            .map(|v| Ok(serde_json::from_value(v)?))
            .collect()
    }

    /// Update the read cursor for an agent to the given event ID.
    ///
    /// Uses UPSERT to create or update in a single atomic operation.
    pub async fn update_read_cursor(
        &self,
        agent_id: &str,
        last_read_event_id: &str,
    ) -> Result<(), PersistenceError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.db
            .query(
                "UPSERT agent_read_cursor SET \
                 agent_id = $aid, \
                 last_read_event_id = $eid, \
                 last_read_at = $now \
                 WHERE agent_id = $aid"
            )
            .bind(("aid", agent_id.to_string()))
            .bind(("eid", last_read_event_id.to_string()))
            .bind(("now", now))
            .await
            .map_err(db_err)?;
        Ok(())
    }

    // =====================================================================
    // Query helpers for NATS data
    // =====================================================================

    /// Get the most recent presence heartbeat for an agent.
    pub async fn get_latest_presence(
        &self,
        agent: &str,
    ) -> Result<Option<Presence>, PersistenceError> {
        let mut result = self.db
            .query(
                "SELECT * OMIT id FROM presence WHERE agent = $agent ORDER BY timestamp DESC LIMIT 1",
            )
            .bind(("agent", agent.to_string()))
            .await
            .map_err(db_err)?;
        let rows: Vec<Value> = result.take(0).map_err(db_err)?;
        match rows.into_iter().next() {
            Some(val) => {
                let p: Presence = serde_json::from_value(val)?;
                Ok(Some(p))
            }
            None => Ok(None),
        }
    }

    /// Get all recent presence records (one per agent, most recent).
    pub async fn get_all_presence(&self) -> Result<Vec<Presence>, PersistenceError> {
        let mut result = self.db
            .query("SELECT * OMIT id FROM presence ORDER BY timestamp DESC")
            .await
            .map_err(db_err)?;
        let rows: Vec<Value> = result.take(0).map_err(db_err)?;

        // Deduplicate by agent — keep only most recent per agent
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for row in rows {
            if let Ok(p) = serde_json::from_value::<Presence>(row) {
                if seen.insert(p.agent.clone()) {
                    out.push(p);
                }
            }
        }
        Ok(out)
    }

    /// Get task assignments for an agent on a project.
    pub async fn get_tasks_for_agent(
        &self,
        agent: &str,
        project: &str,
    ) -> Result<Vec<TaskAssignment>, PersistenceError> {
        let mut result = self.db
            .query(
                "SELECT * OMIT id FROM task_assignment \
                 WHERE assigned_to = $agent AND project = $project \
                 ORDER BY timestamp DESC",
            )
            .bind(("agent", agent.to_string()))
            .bind(("project", project.to_string()))
            .await
            .map_err(db_err)?;
        let rows: Vec<Value> = result.take(0).map_err(db_err)?;
        rows.into_iter()
            .map(|v| Ok(serde_json::from_value(v)?))
            .collect()
    }

    /// Get status updates for a specific task.
    pub async fn get_task_status(
        &self,
        task_id: &str,
    ) -> Result<Vec<TaskStatusUpdate>, PersistenceError> {
        let mut result = self.db
            .query(
                "SELECT * OMIT id FROM task_status_update \
                 WHERE task_id = $tid \
                 ORDER BY timestamp ASC",
            )
            .bind(("tid", task_id.to_string()))
            .await
            .map_err(db_err)?;
        let rows: Vec<Value> = result.take(0).map_err(db_err)?;
        rows.into_iter()
            .map(|v| Ok(serde_json::from_value(v)?))
            .collect()
    }

    /// Get session notes for an agent on a project.
    pub async fn get_session_notes(
        &self,
        agent: &str,
        project: &str,
        limit: usize,
    ) -> Result<Vec<SessionNote>, PersistenceError> {
        let mut result = self.db
            .query(
                "SELECT * OMIT id FROM session_note \
                 WHERE agent = $agent AND project = $project \
                 ORDER BY timestamp DESC \
                 LIMIT $limit",
            )
            .bind(("agent", agent.to_string()))
            .bind(("project", project.to_string()))
            .bind(("limit", limit))
            .await
            .map_err(db_err)?;
        let rows: Vec<Value> = result.take(0).map_err(db_err)?;
        rows.into_iter()
            .map(|v| Ok(serde_json::from_value(v)?))
            .collect()
    }
}

// ==========================================================================
// Helpers
// ==========================================================================

/// Convert a SurrealDB error to our PersistenceError.
fn db_err(e: surrealdb::Error) -> PersistenceError {
    PersistenceError::Database(e.to_string())
}

/// Convert a SurrealDB event row back into an EventEnvelope.
///
/// The row has `event_id` (string) where EventEnvelope expects `id` (Uuid).
/// SurrealDB's record `id` is already omitted via `SELECT * OMIT id`.
fn row_to_envelope(row: Value) -> Result<EventEnvelope, PersistenceError> {
    let mut val = row;
    if let Some(obj) = val.as_object_mut() {
        // Rename event_id → id for deserialization
        if let Some(eid) = obj.remove("event_id") {
            obj.insert("id".to_string(), eid);
        }
    }
    Ok(serde_json::from_value(val)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    use crate::events::{EventPayload, EventType, ExpectedResponse, MessageEvent, MessageIntent, Priority, ProvenanceLevel};

    fn make_test_event(agent: &str, subject: &str) -> EventEnvelope {
        EventEnvelope {
            id: Uuid::now_v7(),
            timestamp: Utc::now(),
            event_type: EventType::MessageSent,
            agent_id: agent.to_string(),
            payload: EventPayload::Message(MessageEvent {
                from: agent.to_string(),
                to: "all".to_string(),
                subject: subject.to_string(),
                content: "test content".to_string(),
                thread_id: None,
                priority: Priority::Normal,
                intent: MessageIntent::Inform,
                expected_response: ExpectedResponse::None,
                require_ack: false,
                claimed_source_model: None,
                claimed_source_runtime: None,
                claimed_source_mode: None,
                verified_source_model: None,
                verified_source_runtime: None,
                verified_source_mode: None,
                source_worktree: None,
                source_session_id: None,
                provenance_level: ProvenanceLevel::default(),
                provenance_issuer: None,
            }),
        }
    }

    #[tokio::test]
    async fn test_persistence_new() {
        let db = Persistence::new().await;
        assert!(db.is_ok(), "Persistence::new() should succeed: {:?}", db.err());
    }

    #[tokio::test]
    async fn test_store_and_get_event() {
        let db = Persistence::new().await.unwrap();
        let event = make_test_event("aleph", "Test message");
        let event_id = event.id.to_string();

        let stored_id = db.store_event(&event).await.unwrap();
        assert_eq!(stored_id, event_id);

        let retrieved = db.get_event(&event_id).await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id.to_string(), event_id);
        assert_eq!(retrieved.agent_id, "aleph");
    }

    #[tokio::test]
    async fn test_get_events_for_agent() {
        let db = Persistence::new().await.unwrap();

        db.store_event(&make_test_event("aleph", "msg1")).await.unwrap();
        db.store_event(&make_test_event("luban", "msg2")).await.unwrap();
        db.store_event(&make_test_event("aleph", "msg3")).await.unwrap();

        let aleph_events = db.get_events_for_agent("aleph", 10).await.unwrap();
        assert_eq!(aleph_events.len(), 2);

        let luban_events = db.get_events_for_agent("luban", 10).await.unwrap();
        assert_eq!(luban_events.len(), 1);
    }

    #[tokio::test]
    async fn test_get_events_by_type() {
        let db = Persistence::new().await.unwrap();

        db.store_event(&make_test_event("aleph", "msg1")).await.unwrap();

        let events = db.get_events_by_type("message_sent", 10).await.unwrap();
        assert_eq!(events.len(), 1);

        let empty = db.get_events_by_type("decision_recorded", 10).await.unwrap();
        assert!(empty.is_empty());
    }

    #[tokio::test]
    async fn test_store_presence() {
        let db = Persistence::new().await.unwrap();
        let p = Presence::new("aleph", "mingqiao", "main", "working");

        db.store_presence(&p).await.unwrap();

        let latest = db.get_latest_presence("aleph").await.unwrap();
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().agent, "aleph");
    }

    #[tokio::test]
    async fn test_store_task_assignment_with_project() {
        let db = Persistence::new().await.unwrap();
        let ta = TaskAssignment {
            task_id: "task-001".to_string(),
            title: "Build persistence".to_string(),
            assigned_by: "aleph".to_string(),
            assigned_to: "luban".to_string(),
            spec: "Implement SurrealDB layer".to_string(),
            expected_outputs: vec!["src/db/persistence.rs".to_string()],
            boundaries: vec![],
            priority: Priority::Normal,
            timestamp: Utc::now(),
        };

        db.store_task_assignment(&ta, "mingqiao").await.unwrap();

        let tasks = db.get_tasks_for_agent("luban", "mingqiao").await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].task_id, "task-001");
    }

    #[tokio::test]
    async fn test_store_session_note() {
        let db = Persistence::new().await.unwrap();
        let sn = SessionNote {
            agent: "aleph".to_string(),
            project: "mingqiao".to_string(),
            branch: "main".to_string(),
            completed: vec!["task 1".to_string()],
            in_progress: vec![],
            decisions: vec![],
            unresolved: vec![],
            next_session: vec![],
            timestamp: Utc::now(),
        };

        db.store_session_note(&sn).await.unwrap();

        let notes = db.get_session_notes("aleph", "mingqiao", 10).await.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].completed, vec!["task 1".to_string()]);
    }

    #[tokio::test]
    async fn test_get_all_presence() {
        let db = Persistence::new().await.unwrap();

        db.store_presence(&Presence::new("aleph", "mingqiao", "main", "working")).await.unwrap();
        db.store_presence(&Presence::new("luban", "mingqiao", "main", "idle")).await.unwrap();
        db.store_presence(&Presence::new("aleph", "mingqiao", "main", "updated")).await.unwrap();

        let all = db.get_all_presence().await.unwrap();
        // Should deduplicate — one entry per agent
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_get_latest_presence() {
        let db = Persistence::new().await.unwrap();

        db.store_presence(&Presence::new("aleph", "mingqiao", "main", "first")).await.unwrap();
        // Small delay to ensure different timestamps
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        db.store_presence(&Presence::new("aleph", "mingqiao", "main", "second")).await.unwrap();

        let latest = db.get_latest_presence("aleph").await.unwrap().unwrap();
        assert_eq!(latest.status, "second");
    }

    #[tokio::test]
    async fn test_get_all_events() {
        let db = Persistence::new().await.unwrap();

        // Empty DB returns empty vec
        let empty = db.get_all_events().await.unwrap();
        assert!(empty.is_empty());

        // Store 3 events with small delays to ensure distinct timestamps
        db.store_event(&make_test_event("aleph", "first")).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        db.store_event(&make_test_event("luban", "second")).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        db.store_event(&make_test_event("thales", "third")).await.unwrap();

        let all = db.get_all_events().await.unwrap();
        assert_eq!(all.len(), 3);

        // Verify ascending timestamp order
        assert!(all[0].timestamp <= all[1].timestamp);
        assert!(all[1].timestamp <= all[2].timestamp);

        // Verify all agents present
        assert_eq!(all[0].agent_id, "aleph");
        assert_eq!(all[1].agent_id, "luban");
        assert_eq!(all[2].agent_id, "thales");
    }

    #[tokio::test]
    async fn test_get_event_not_found() {
        let db = Persistence::new().await.unwrap();
        let result = db.get_event("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_read_cursor_roundtrip() {
        let db = Persistence::new().await.unwrap();

        // No cursor initially
        let cursor = db.get_read_cursor("aleph").await.unwrap();
        assert!(cursor.is_none());

        // Set cursor
        db.update_read_cursor("aleph", "event-001").await.unwrap();
        let cursor = db.get_read_cursor("aleph").await.unwrap();
        assert_eq!(cursor.as_deref(), Some("event-001"));

        // Advance cursor (upsert)
        db.update_read_cursor("aleph", "event-005").await.unwrap();
        let cursor = db.get_read_cursor("aleph").await.unwrap();
        assert_eq!(cursor.as_deref(), Some("event-005"));

        // Different agent has no cursor
        let other = db.get_read_cursor("luban").await.unwrap();
        assert!(other.is_none());
    }
}
