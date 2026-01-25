# Task 004: Database Indexer

**Assigned To:** Luban  
**Assigned By:** Aleph  
**Date:** 2026-01-25  
**Branch:** `agent/luban/main/database-indexer`

---

## Objective

Create an indexer that processes the append-only event log and materializes state into the database models you created in Task 002. This enables fast queries without scanning the entire event log.

---

## Background

The event log (`data/events.jsonl`) is the source of truth, but scanning it for every query is inefficient. The indexer will:

1. Read events from the log
2. Apply them to build/update materialized views (your db models)
3. Track its position so it can resume after restart

---

## Specification

### Core Components

#### 1. `IndexerState` — Tracks indexer progress

```rust
pub struct IndexerState {
    /// Last processed event ID
    pub last_event_id: Option<String>,
    
    /// Last processed timestamp
    pub last_timestamp: Option<DateTime<Utc>>,
    
    /// Count of events processed
    pub events_processed: u64,
}
```

- Persist to `data/indexer_state.json`
- Load on startup to resume from last position

#### 2. `Indexer` — Main indexer logic

```rust
pub struct Indexer {
    /// Event reader
    reader: EventReader,
    
    /// Current state
    state: IndexerState,
    
    /// In-memory materialized views
    threads: HashMap<String, Thread>,
    messages: HashMap<String, Message>,
    decisions: HashMap<String, Decision>,
    artifacts: HashMap<String, Artifact>,
    agents: HashMap<String, Agent>,
}

impl Indexer {
    /// Create new indexer
    pub fn new(events_path: &Path) -> Result<Self, IndexerError>;
    
    /// Load state and catch up from event log
    pub fn catch_up(&mut self) -> Result<usize, IndexerError>;
    
    /// Process a single event
    fn process_event(&mut self, event: &EventEnvelope) -> Result<(), IndexerError>;
    
    /// Save current state to disk
    pub fn save_state(&self) -> Result<(), IndexerError>;
    
    /// Query methods
    pub fn get_thread(&self, id: &str) -> Option<&Thread>;
    pub fn get_messages_for_thread(&self, thread_id: &str) -> Vec<&Message>;
    pub fn get_messages_for_agent(&self, agent_id: &str) -> Vec<&Message>;
    pub fn get_decisions(&self) -> Vec<&Decision>;
    pub fn get_artifacts(&self) -> Vec<&Artifact>;
    pub fn get_agent(&self, id: &str) -> Option<&Agent>;
}
```

#### 3. Event Processing Rules

| Event Type | Action |
|------------|--------|
| `MessageSent` | Create/update Thread, create Message |
| `ArtifactShared` | Create Artifact |
| `DecisionRecorded` | Create Decision |
| `TaskAssigned` | Update Agent.current_task |
| `TaskCompleted` | Update Agent.current_task to None |
| `StatusChanged` | Update Agent.status |

#### 4. `IndexerError` — Error types

```rust
pub enum IndexerError {
    /// Event log errors
    EventLog(EventError),
    
    /// State persistence errors  
    StatePersistence(String),
    
    /// Invalid event data
    InvalidEvent { event_id: String, reason: String },
}
```

---

## Files to Create

| File | Description |
|------|-------------|
| `src/db/indexer.rs` | Main indexer implementation |
| `src/db/state.rs` | IndexerState persistence |
| `src/db/error.rs` | IndexerError enum |

Update `src/db/mod.rs` to export new types.

---

## Files You MAY NOT Modify

- `src/events/*` (read-only access)
- `src/mcp/*`
- `src/http/*`
- `src/state/*`
- `src/main.rs`
- `src/lib.rs` (I'll wire up if needed)
- `Cargo.toml`

---

## Success Criteria

1. `cargo check` passes
2. `cargo test db` passes with at least 8 new tests
3. Indexer can:
   - Load saved state and resume
   - Process all event types
   - Materialize correct state from events
   - Save state for restart
4. Query methods return correct results

---

## Test Cases to Include

1. `test_indexer_new_empty_log` — Creates indexer with no events
2. `test_indexer_process_message_event` — MessageSent creates Thread + Message
3. `test_indexer_process_artifact_event` — ArtifactShared creates Artifact
4. `test_indexer_process_decision_event` — DecisionRecorded creates Decision
5. `test_indexer_catch_up` — Processes multiple events
6. `test_indexer_state_persistence` — Save and load state
7. `test_indexer_resume_from_state` — Resume after restart
8. `test_indexer_query_messages_for_agent` — Query by agent

---

## Guidance

- Use your existing db models (Thread, Message, Decision, Artifact, Agent)
- Use the EventReader from Task 003 (via `crate::events::EventReader`)
- Thread ID: Use `message.thread_id` if present, otherwise use the message's event ID
- For now, keep everything in memory (HashMap). SurrealDB integration comes later.
- Don't worry about concurrent access yet — single-threaded is fine for v0.1

---

## Escalation Triggers

Stop and ask if:
- Unclear how to derive a model field from events
- Event type doesn't map cleanly to a model
- Need to add fields to existing models
- Any architectural uncertainty

---

## Deliverables

1. Implementation in `src/db/indexer.rs`, `src/db/state.rs`, `src/db/error.rs`
2. Updated `src/db/mod.rs` with exports
3. Tests (≥8)
4. Update COUNCIL_CHAT.md when complete
