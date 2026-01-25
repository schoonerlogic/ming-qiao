# Task Assignment: Event Schema Foundation

**From:** Aleph  
**To:** Luban  
**Date:** 2026-01-24  
**Priority:** High (blocking other work)

---

## TASK ASSIGNMENT: Event Schema Foundation

**Objective:** Implement the core event types that form the foundation of ming-qiao's append-only event log.

---

### Specification

Create the event schema in `src/events/schema.rs` with the following types:

#### 1. Base Event Envelope

```rust
/// All events share this envelope structure
pub struct EventEnvelope {
    pub id: String,           // UUID v7 (time-sortable)
    pub timestamp: DateTime<Utc>,
    pub event_type: EventType,
    pub agent_id: String,     // Which agent produced this event
    pub payload: EventPayload,
}
```

#### 2. Event Types Enum

```rust
pub enum EventType {
    MessageSent,
    MessageReceived,
    ArtifactShared,
    DecisionRecorded,
    TaskAssigned,
    TaskCompleted,
    StatusChanged,
}
```

#### 3. Event Payloads

```rust
pub enum EventPayload {
    Message(MessageEvent),
    Artifact(ArtifactEvent),
    Decision(DecisionEvent),
    Task(TaskEvent),
    Status(StatusEvent),
}

pub struct MessageEvent {
    pub from: String,
    pub to: String,
    pub subject: String,
    pub content: String,
    pub thread_id: Option<String>,
    pub priority: Priority,
}

pub struct ArtifactEvent {
    pub path: String,
    pub description: String,
    pub checksum: String,
}

pub struct DecisionEvent {
    pub title: String,
    pub context: String,
    pub options: Vec<DecisionOption>,
    pub chosen: usize,
    pub rationale: String,
}

pub struct DecisionOption {
    pub description: String,
    pub pros: Vec<String>,
    pub cons: Vec<String>,
}

pub struct TaskEvent {
    pub task_id: String,
    pub title: String,
    pub assigned_to: String,
    pub assigned_by: String,
    pub status: TaskStatus,
}

pub struct StatusEvent {
    pub agent_id: String,
    pub previous: AgentStatus,
    pub current: AgentStatus,
    pub reason: Option<String>,
}
```

#### 4. Supporting Enums

```rust
pub enum Priority {
    Low,
    Normal,
    High,
    Critical,
}

pub enum TaskStatus {
    Assigned,
    InProgress,
    Blocked,
    Ready,
    Completed,
    Cancelled,
}

pub enum AgentStatus {
    Available,
    Working,
    Blocked,
    Offline,
}
```

---

### Inputs Provided

- This specification document
- `Cargo.toml` already includes: `serde`, `chrono`, `uuid`

---

### Expected Outputs

1. **File:** `src/events/schema.rs`
   - All types above implemented
   - Derive macros: `Debug, Clone, Serialize, Deserialize` on all types
   - Documentation comments on each struct and enum

2. **File:** `src/events/mod.rs`
   - Module declaration and public exports

3. **File:** `src/events/tests.rs`
   - Test: `EventEnvelope` serializes to JSON and back
   - Test: Each `EventPayload` variant round-trips correctly
   - Test: `EventType` and `Priority` enums serialize as expected strings

---

### Boundaries

**Files to create/modify:**
- `src/events/schema.rs` ✅
- `src/events/mod.rs` ✅
- `src/events/tests.rs` ✅

**Files NOT to touch:**
- `src/mcp/*`
- `src/http/*`
- `src/lib.rs` (I will wire up the module)
- `Cargo.toml`

---

### Success Criteria

- [ ] `cargo check` passes
- [ ] `cargo test events` passes
- [ ] All types have doc comments
- [ ] JSON serialization produces readable output (snake_case fields, string enums)

---

### Escalation Triggers

Stop and ask Aleph if:
- You need additional dependencies
- The spec is ambiguous or contradictory
- You encounter a design decision not covered here
- Tests reveal issues with the spec itself

---

### Confirmation Required

Before starting implementation, respond with:

```
TASK RECEIVED: Event Schema Foundation

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
