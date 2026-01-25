# Task Assignment: Event Persistence Layer

**Assigned to:** Luban  
**Priority:** P1 (High — blocking MCP/HTTP integration)  
**Status:** Pending  
**Created:** 2026-01-25

---

## Objective

Implement the event writer and reader for ming-qiao's append-only event log. This is the source of truth — all state is derived from replaying events.

---

## Deliverables

1. [ ] `src/events/writer.rs` — Append-only event writer
2. [ ] `src/events/reader.rs` — Event log reader (tail/replay)
3. [ ] `src/events/mod.rs` — Update module exports
4. [ ] `src/events/tests.rs` — Add persistence tests (or new test file)

---

## Specification

### Context

Events are stored as newline-delimited JSON (JSONL) in `data/events.jsonl`. Each line is a complete `EventEnvelope` serialized as JSON. The writer appends events atomically; the reader can replay from start or tail for new events.

### Event Writer

```rust
/// Append-only event writer
pub struct EventWriter {
    path: PathBuf,
    // Internal file handle or write strategy
}

impl EventWriter {
    /// Create a new writer for the given path
    /// Creates parent directories and file if they don't exist
    pub fn new(path: impl AsRef<Path>) -> Result<Self, EventError>;
    
    /// Append an event to the log
    /// Returns the event ID after successful write
    pub fn append(&mut self, event: &EventEnvelope) -> Result<String, EventError>;
    
    /// Flush any buffered writes to disk
    pub fn flush(&mut self) -> Result<(), EventError>;
    
    /// Get the current file size in bytes
    pub fn size(&self) -> Result<u64, EventError>;
}
```

### Event Reader

```rust
/// Event log reader with replay and tail capabilities
pub struct EventReader {
    path: PathBuf,
    // Internal state for position tracking
}

impl EventReader {
    /// Open an event log for reading
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EventError>;
    
    /// Replay all events from the beginning
    /// Returns an iterator over events
    pub fn replay(&self) -> Result<impl Iterator<Item = Result<EventEnvelope, EventError>>, EventError>;
    
    /// Read events after a given event ID
    /// Useful for incremental sync
    pub fn after(&self, event_id: &str) -> Result<impl Iterator<Item = Result<EventEnvelope, EventError>>, EventError>;
    
    /// Get count of events in the log
    pub fn count(&self) -> Result<usize, EventError>;
    
    /// Check if log file exists
    pub fn exists(&self) -> bool;
}
```

### Error Type

```rust
/// Errors that can occur during event persistence
#[derive(Debug, thiserror::Error)]
pub enum EventError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("Event not found: {0}")]
    NotFound(String),
    
    #[error("Invalid event log format at line {line}: {message}")]
    InvalidFormat { line: usize, message: String },
}
```

### Requirements

1. **Atomic appends** — Each event write must be atomic (write + newline + flush)
2. **No partial writes** — A crash should never leave partial JSON in the file
3. **Readable format** — One JSON object per line, human-readable
4. **Efficient replay** — Streaming iterator, don't load entire file into memory
5. **Thread-safe** — Writer should handle concurrent append requests (use mutex or channel)

### File Format

```jsonl
{"id":"01234567-...","timestamp":"2026-01-25T10:00:00Z","event_type":"message_sent","agent_id":"aleph","payload":{...}}
{"id":"01234568-...","timestamp":"2026-01-25T10:01:00Z","event_type":"message_received","agent_id":"thales","payload":{...}}
```

- One `EventEnvelope` per line
- No pretty-printing (compact JSON)
- Newline (`\n`) terminates each record
- UTF-8 encoding

---

## Inputs Provided

- `src/events/schema.rs` — Your previous work (EventEnvelope, all event types)
- Architecture doc: `docs/ARCHITECTURE.md`
- This specification

---

## File Boundaries

### Create/Modify
- `src/events/writer.rs` — implement EventWriter
- `src/events/reader.rs` — implement EventReader  
- `src/events/mod.rs` — add module declarations and re-exports
- `src/events/tests.rs` — add persistence tests (or create `persistence_tests.rs`)

### Do NOT Touch
- `src/events/schema.rs` — read-only reference
- `src/db/*` — your previous work, separate concern
- `src/mcp/*` — Aleph's domain
- `src/http/*` — Aleph's domain
- `src/lib.rs` — I will wire up if needed
- `Cargo.toml` — no new dependencies needed (we have std::fs, serde_json)

---

## Success Criteria

- [ ] `cargo check` passes
- [ ] `cargo test events` passes (existing + new tests)
- [ ] Writer can append events and they persist across restarts
- [ ] Reader can replay all events in order
- [ ] Reader can tail events after a given ID
- [ ] Error handling is comprehensive (no panics on bad input)
- [ ] At least 6 new tests for persistence

---

## Escalation Triggers

Stop and ask Aleph if:
- You need to modify `schema.rs`
- You're unsure about atomicity guarantees
- You need additional dependencies
- The spec conflicts with existing event types

---

## Confirmation Required

Before starting, respond in COUNCIL_CHAT.md:

```
TASK RECEIVED: Event Persistence Layer

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
