# Task Assignment: Database Models

**Assigned to:** Luban  
**Priority:** P1 (High — blocking persistence layer)  
**Status:** Pending  
**Created:** 2026-01-25

---

## Objective

Implement the database models that will store materialized views of events in SurrealDB. These models represent the queryable state derived from the append-only event log.

---

## Deliverables

1. [ ] `src/db/models.rs` — All database model structs
2. [ ] `src/db/mod.rs` — Module declaration and re-exports
3. [ ] `src/db/tests.rs` — Serialization tests for models

---

## Specification

### Context

The event log (`src/events/schema.rs`) captures raw events. The database models represent **materialized state** — threads, messages, decisions — that are rebuilt by replaying events. These models map to SurrealDB tables.

### Input

- Event types from `src/events/schema.rs` (your previous work)
- Architecture doc: `docs/ARCHITECTURE.md` (Database section)

### Models to Implement

#### 1. Thread Model

```rust
/// A conversation thread between agents
pub struct Thread {
    pub id: String,              // UUID v7
    pub subject: String,
    pub participants: Vec<String>, // agent IDs
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: u32,
    pub status: ThreadStatus,
}

pub enum ThreadStatus {
    Active,
    Paused,      // Merlin paused it
    Resolved,
    Archived,
}
```

#### 2. Message Model

```rust
/// A message within a thread (materialized from MessageEvent)
pub struct Message {
    pub id: String,              // UUID v7, same as event ID
    pub thread_id: String,
    pub from: String,            // agent ID
    pub to: String,              // agent ID or "all"
    pub subject: String,
    pub content: String,
    pub priority: Priority,      // re-use from events::schema
    pub created_at: DateTime<Utc>,
    pub read_by: Vec<String>,    // agent IDs who have read this
}
```

#### 3. Decision Model

```rust
/// A recorded decision (materialized from DecisionEvent)
pub struct Decision {
    pub id: String,              // UUID v7
    pub thread_id: Option<String>, // which thread spawned this
    pub title: String,
    pub context: String,
    pub options: Vec<DecisionOption>, // re-use from events::schema
    pub chosen: usize,
    pub rationale: String,
    pub status: DecisionStatus,
    pub created_at: DateTime<Utc>,
    pub recorded_by: String,     // agent ID
}

pub enum DecisionStatus {
    Pending,     // awaiting Merlin approval (gated mode)
    Approved,
    Rejected,
    Superseded,  // replaced by newer decision
}
```

#### 4. Artifact Model

```rust
/// A shared artifact (materialized from ArtifactEvent)
pub struct Artifact {
    pub id: String,              // UUID v7
    pub path: String,
    pub description: String,
    pub checksum: String,
    pub shared_by: String,       // agent ID
    pub shared_at: DateTime<Utc>,
    pub thread_id: Option<String>, // context thread if any
}
```

#### 5. Agent Model

```rust
/// Current state of an agent (materialized from StatusEvents)
pub struct Agent {
    pub id: String,              // "aleph", "luban", "thales", etc.
    pub display_name: String,
    pub status: AgentStatus,     // re-use from events::schema
    pub last_seen: DateTime<Utc>,
    pub current_task: Option<String>,
}
```

#### 6. Annotation Model

```rust
/// Merlin's notes on threads or decisions
pub struct Annotation {
    pub id: String,              // UUID v7
    pub target_type: AnnotationTarget,
    pub target_id: String,       // thread_id or decision_id
    pub content: String,
    pub created_at: DateTime<Utc>,
}

pub enum AnnotationTarget {
    Thread,
    Decision,
    Message,
}
```

### Requirements

1. All models must derive: `Debug, Clone, Serialize, Deserialize`
2. All models must have doc comments
3. Enums serialize as lowercase snake_case strings
4. Re-use types from `src/events/schema.rs` where indicated (Priority, AgentStatus, DecisionOption)
5. All IDs are String (UUID v7 format, but stored as string for SurrealDB compatibility)

---

## File Boundaries

### Create/Modify
- `src/db/models.rs` — implement all models
- `src/db/mod.rs` — module structure
- `src/db/tests.rs` — serialization tests

### Do NOT Touch
- `src/events/*` — your previous work, read-only reference
- `src/mcp/*` — Aleph's domain
- `src/lib.rs` — I will wire up the module
- `Cargo.toml` — no new dependencies needed

---

## Success Criteria

- [ ] `cargo check` passes
- [ ] `cargo test db` passes (at least 6 tests — one per model)
- [ ] All models have doc comments
- [ ] JSON serialization produces readable output
- [ ] Types from `events::schema` are properly imported and re-used

---

## Escalation Triggers

Stop and ask Aleph if:
- You need to modify `events::schema.rs`
- The spec conflicts with existing event types
- You discover a model relationship not covered here
- SurrealDB requires specific annotations not mentioned

---

## Confirmation Required

Before starting, respond in COUNCIL_CHAT.md:

```
TASK RECEIVED: Database Models

My understanding:
- Input: <summarize>
- Output: <summarize>
- Scope: <files>
- Constraints: <limitations>
- Success criteria: <how to verify>

Questions before starting:
- <any clarifications>

Ready to proceed? [waiting for confirmation]
```

---

**Aleph**  
Master Builder, Council of Wizards
