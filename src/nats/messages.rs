//! Typed NATS message payloads for agent coordination
//!
//! Each message type corresponds to a subject category in the hierarchy:
//!
//! - [`Presence`] → `am.agent.{agent}.presence` (core NATS, ephemeral)
//! - [`TaskAssignment`] → `am.agent.{agent}.task.{project}.assigned` (JetStream)
//! - [`TaskStatusUpdate`] → `am.agent.{agent}.task.{project}.*` (JetStream)
//! - [`SessionNote`] → `am.agent.{agent}.notes.{project}` (JetStream)
//!
//! All payloads are JSON-serialized. Types reuse the existing event schema
//! enums (`Priority`, `TaskStatus`) where applicable — no schema invention.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::events::{Priority, TaskStatus};

// ============================================================================
// Presence (core NATS — ephemeral)
// ============================================================================

/// Heartbeat message published at regular intervals.
///
/// Published to `am.agent.{agent}.presence` using core NATS (no JetStream).
/// Subscribers maintain a local roster of who's online by watching
/// `am.agent.*.presence` and applying a staleness timeout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Presence {
    /// Agent identifier (e.g., "aleph", "luban")
    pub agent: String,

    /// Project the agent is currently working on
    pub project: String,

    /// Git branch the agent is currently on
    pub branch: String,

    /// What the agent is currently doing (free-form status line)
    pub status: String,

    /// Timestamp of this heartbeat
    pub timestamp: DateTime<Utc>,
}

impl Presence {
    /// Create a new presence heartbeat for the current moment.
    pub fn new(
        agent: impl Into<String>,
        project: impl Into<String>,
        branch: impl Into<String>,
        status: impl Into<String>,
    ) -> Self {
        Self {
            agent: agent.into(),
            project: project.into(),
            branch: branch.into(),
            status: status.into(),
            timestamp: Utc::now(),
        }
    }
}

// ============================================================================
// Task coordination (JetStream — persistent, work queue)
// ============================================================================

/// Task assignment message.
///
/// Published to `am.agent.{assignee}.task.{project}.assigned` when one agent
/// assigns work to another. The assignee's durable consumer picks this up
/// exactly once (work queue delivery policy).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAssignment {
    /// Unique task identifier (UUID v7)
    pub task_id: String,

    /// Human-readable task title
    pub title: String,

    /// Agent assigning the task
    pub assigned_by: String,

    /// Agent receiving the task
    pub assigned_to: String,

    /// Detailed task specification (markdown)
    pub spec: String,

    /// Files the assignee should create or modify
    #[serde(default)]
    pub expected_outputs: Vec<String>,

    /// Files the assignee must NOT touch
    #[serde(default)]
    pub boundaries: Vec<String>,

    /// Task priority
    #[serde(default)]
    pub priority: Priority,

    /// When the task was assigned
    pub timestamp: DateTime<Utc>,
}

/// Task status update message.
///
/// Published to the appropriate subtopic based on the new status:
/// - `.started` → agent began work
/// - `.update` → progress report
/// - `.complete` → work finished
/// - `.blocked` → agent is stuck
///
/// Both the assignee and assigner subscribe to `am.agent.*.task.{project}.>`
/// for full visibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatusUpdate {
    /// Task identifier this update applies to
    pub task_id: String,

    /// Agent publishing the update
    pub agent: String,

    /// New status
    pub status: TaskStatus,

    /// Human-readable summary of what changed
    pub summary: String,

    /// Blocker description (when status is Blocked)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocker: Option<String>,

    /// Files modified so far (for progress updates and completions)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files_changed: Vec<String>,

    /// When this update was published
    pub timestamp: DateTime<Utc>,
}

impl TaskStatusUpdate {
    /// Which task subject subtopic this update should be published on.
    ///
    /// Maps `TaskStatus` to the NATS subject suffix:
    /// - `Assigned` → `"assigned"` (unusual — normally comes from TaskAssignment)
    /// - `InProgress` → `"started"`
    /// - `Blocked` → `"blocked"`
    /// - `Ready` → `"complete"`
    /// - `Completed` → `"complete"`
    /// - `Cancelled` → `"complete"`
    pub fn subject_suffix(&self) -> &'static str {
        match self.status {
            TaskStatus::Assigned => "assigned",
            TaskStatus::InProgress => "started",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Ready | TaskStatus::Completed | TaskStatus::Cancelled => "complete",
        }
    }
}

// ============================================================================
// Session notes (JetStream — persistent, 30-day retention)
// ============================================================================

/// Session summary published at session end or periodically.
///
/// Published to `am.agent.{agent}.notes.{project}`. Multiple observers
/// (including Laozi-Jung) subscribe to `am.agent.*.notes.>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionNote {
    /// Agent publishing the notes
    pub agent: String,

    /// Project these notes relate to
    pub project: String,

    /// Git branch the agent was working on
    pub branch: String,

    /// Summary of what was accomplished
    pub completed: Vec<String>,

    /// Work still in progress
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub in_progress: Vec<String>,

    /// Decisions made during this session
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<String>,

    /// Unresolved questions or blockers
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unresolved: Vec<String>,

    /// What the next session should prioritize
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_session: Vec<String>,

    /// When the session ended
    pub timestamp: DateTime<Utc>,
}

// ============================================================================
// Envelope — wraps any message for NATS transport
// ============================================================================

/// Wrapper enum for all NATS message types.
///
/// Allows generic publish/subscribe handlers to deserialize any message
/// from the bus without knowing the subject in advance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum NatsMessage {
    Presence(Presence),
    TaskAssignment(TaskAssignment),
    TaskStatusUpdate(TaskStatusUpdate),
    SessionNote(SessionNote),
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Presence
    // ========================================================================

    #[test]
    fn test_presence_serialization_round_trip() {
        let p = Presence::new("aleph", "mingqiao", "agent/aleph/nats-bridge", "implementing subjects.rs");

        let json = serde_json::to_string(&p).unwrap();
        let deser: Presence = serde_json::from_str(&json).unwrap();

        assert_eq!(deser.agent, "aleph");
        assert_eq!(deser.project, "mingqiao");
        assert_eq!(deser.branch, "agent/aleph/nats-bridge");
        assert_eq!(deser.status, "implementing subjects.rs");
    }

    #[test]
    fn test_presence_has_timestamp() {
        let p = Presence::new("luban", "mingqiao", "main", "available");
        assert!(p.timestamp <= Utc::now());
    }

    // ========================================================================
    // Task Assignment
    // ========================================================================

    #[test]
    fn test_task_assignment_serialization() {
        let ta = TaskAssignment {
            task_id: "019c0000-0000-7000-8000-000000000001".to_string(),
            title: "Implement NATS subjects".to_string(),
            assigned_by: "aleph".to_string(),
            assigned_to: "luban".to_string(),
            spec: "Create src/nats/subjects.rs with AgentSubjects struct".to_string(),
            expected_outputs: vec!["src/nats/subjects.rs".to_string()],
            boundaries: vec!["src/events/*".to_string()],
            priority: Priority::Normal,
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&ta).unwrap();
        let deser: TaskAssignment = serde_json::from_str(&json).unwrap();

        assert_eq!(deser.task_id, ta.task_id);
        assert_eq!(deser.assigned_by, "aleph");
        assert_eq!(deser.assigned_to, "luban");
        assert_eq!(deser.priority, Priority::Normal);
        assert_eq!(deser.expected_outputs.len(), 1);
        assert_eq!(deser.boundaries.len(), 1);
    }

    #[test]
    fn test_task_assignment_defaults() {
        // Minimal assignment (no expected_outputs or boundaries)
        let json = r#"{
            "task_id": "test-123",
            "title": "Test task",
            "assigned_by": "aleph",
            "assigned_to": "luban",
            "spec": "Do the thing",
            "priority": "high",
            "timestamp": "2026-02-19T00:00:00Z"
        }"#;

        let ta: TaskAssignment = serde_json::from_str(json).unwrap();
        assert!(ta.expected_outputs.is_empty());
        assert!(ta.boundaries.is_empty());
        assert_eq!(ta.priority, Priority::High);
    }

    // ========================================================================
    // Task Status Update
    // ========================================================================

    #[test]
    fn test_task_status_update_serialization() {
        let update = TaskStatusUpdate {
            task_id: "test-123".to_string(),
            agent: "luban".to_string(),
            status: TaskStatus::InProgress,
            summary: "Started working on subjects.rs".to_string(),
            blocker: None,
            files_changed: vec![],
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&update).unwrap();
        let deser: TaskStatusUpdate = serde_json::from_str(&json).unwrap();

        assert_eq!(deser.task_id, "test-123");
        assert_eq!(deser.status, TaskStatus::InProgress);
        assert!(deser.blocker.is_none());
    }

    #[test]
    fn test_task_status_update_with_blocker() {
        let update = TaskStatusUpdate {
            task_id: "test-123".to_string(),
            agent: "luban".to_string(),
            status: TaskStatus::Blocked,
            summary: "Need type definitions from Aleph".to_string(),
            blocker: Some("Waiting for NatsConfig struct definition".to_string()),
            files_changed: vec!["src/nats/bridge.rs".to_string()],
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&update).unwrap();
        assert!(json.contains("blocker"));

        let deser: TaskStatusUpdate = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.blocker.as_deref(), Some("Waiting for NatsConfig struct definition"));
        assert_eq!(deser.files_changed.len(), 1);
    }

    #[test]
    fn test_task_status_subject_suffix() {
        let make = |status: TaskStatus| TaskStatusUpdate {
            task_id: "t".to_string(),
            agent: "a".to_string(),
            status,
            summary: String::new(),
            blocker: None,
            files_changed: vec![],
            timestamp: Utc::now(),
        };

        assert_eq!(make(TaskStatus::Assigned).subject_suffix(), "assigned");
        assert_eq!(make(TaskStatus::InProgress).subject_suffix(), "started");
        assert_eq!(make(TaskStatus::Blocked).subject_suffix(), "blocked");
        assert_eq!(make(TaskStatus::Ready).subject_suffix(), "complete");
        assert_eq!(make(TaskStatus::Completed).subject_suffix(), "complete");
        assert_eq!(make(TaskStatus::Cancelled).subject_suffix(), "complete");
    }

    // ========================================================================
    // Session Note
    // ========================================================================

    #[test]
    fn test_session_note_serialization() {
        let note = SessionNote {
            agent: "aleph".to_string(),
            project: "mingqiao".to_string(),
            branch: "agent/aleph/nats-bridge".to_string(),
            completed: vec![
                "NATS JetStream bridge implemented".to_string(),
                "Integration test passed".to_string(),
            ],
            in_progress: vec!["Agent client refactor".to_string()],
            decisions: vec!["Option B: purpose-specific subjects only".to_string()],
            unresolved: vec![],
            next_session: vec!["Implement messages.rs".to_string()],
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&note).unwrap();
        let deser: SessionNote = serde_json::from_str(&json).unwrap();

        assert_eq!(deser.agent, "aleph");
        assert_eq!(deser.completed.len(), 2);
        assert_eq!(deser.in_progress.len(), 1);
        assert_eq!(deser.decisions.len(), 1);
        assert!(deser.unresolved.is_empty());
        assert_eq!(deser.next_session.len(), 1);
    }

    #[test]
    fn test_session_note_empty_optional_fields_omitted() {
        let note = SessionNote {
            agent: "luban".to_string(),
            project: "mingqiao".to_string(),
            branch: "main".to_string(),
            completed: vec!["Task done".to_string()],
            in_progress: vec![],
            decisions: vec![],
            unresolved: vec![],
            next_session: vec![],
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&note).unwrap();
        // Empty vecs should be omitted from JSON (skip_serializing_if)
        assert!(!json.contains("in_progress"));
        assert!(!json.contains("decisions"));
        assert!(!json.contains("unresolved"));
        assert!(!json.contains("next_session"));
        // Non-empty fields should be present
        assert!(json.contains("completed"));
    }

    // ========================================================================
    // NatsMessage envelope
    // ========================================================================

    #[test]
    fn test_nats_message_presence_variant() {
        let msg = NatsMessage::Presence(Presence::new("aleph", "mingqiao", "main", "idle"));
        let json = serde_json::to_string(&msg).unwrap();

        assert!(json.contains(r#""type":"presence"#));
        assert!(json.contains(r#""data":{"#));

        let deser: NatsMessage = serde_json::from_str(&json).unwrap();
        match deser {
            NatsMessage::Presence(p) => assert_eq!(p.agent, "aleph"),
            _ => panic!("Expected Presence variant"),
        }
    }

    #[test]
    fn test_nats_message_task_assignment_variant() {
        let msg = NatsMessage::TaskAssignment(TaskAssignment {
            task_id: "t-1".to_string(),
            title: "Test".to_string(),
            assigned_by: "aleph".to_string(),
            assigned_to: "luban".to_string(),
            spec: "Do it".to_string(),
            expected_outputs: vec![],
            boundaries: vec![],
            priority: Priority::Normal,
            timestamp: Utc::now(),
        });

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"task_assignment"#));

        let deser: NatsMessage = serde_json::from_str(&json).unwrap();
        match deser {
            NatsMessage::TaskAssignment(ta) => assert_eq!(ta.assigned_to, "luban"),
            _ => panic!("Expected TaskAssignment variant"),
        }
    }

    #[test]
    fn test_nats_message_session_note_variant() {
        let msg = NatsMessage::SessionNote(SessionNote {
            agent: "aleph".to_string(),
            project: "mingqiao".to_string(),
            branch: "main".to_string(),
            completed: vec!["done".to_string()],
            in_progress: vec![],
            decisions: vec![],
            unresolved: vec![],
            next_session: vec![],
            timestamp: Utc::now(),
        });

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"session_note"#));
    }

    // ========================================================================
    // Reuse of existing schema types
    // ========================================================================

    #[test]
    fn test_priority_reuse_from_event_schema() {
        // Verify Priority serializes the same way as in events::schema
        let json = serde_json::to_string(&Priority::Critical).unwrap();
        assert_eq!(json, r#""critical""#);
    }

    #[test]
    fn test_task_status_reuse_from_event_schema() {
        // Verify TaskStatus serializes the same way as in events::schema
        let json = serde_json::to_string(&TaskStatus::InProgress).unwrap();
        assert_eq!(json, r#""in_progress""#);
    }
}
