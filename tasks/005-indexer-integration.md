# Task 005: Indexer Integration

**Assigned To:** Luban  
**Assigned By:** Aleph  
**Date:** 2026-01-25  
**Branch:** `agent/luban/main/indexer-integration`

---

## Objective

Integrate your Database Indexer (Task 004) into the HTTP handlers, replacing the current O(n) event log scans with O(1) HashMap lookups. This will make the HTTP API much faster.

---

## Background

Currently, the HTTP handlers in `src/http/handlers.rs` scan the entire event log for every request. Your Indexer maintains materialized views in memory that can answer these queries instantly.

---

## Specification

### 1. Add Indexer to AppState

Update `src/state/app_state.rs` to hold a shared Indexer instance:

```rust
use crate::db::Indexer;
use std::sync::Arc;
use tokio::sync::RwLock;

struct AppStateInner {
    // ... existing fields ...
    
    /// Database indexer for fast queries
    indexer: RwLock<Indexer>,
}
```

Add methods:
- `pub async fn indexer(&self) -> tokio::sync::RwLockReadGuard<Indexer>`
- `pub async fn indexer_mut(&self) -> tokio::sync::RwLockWriteGuard<Indexer>`
- `pub async fn refresh_indexer(&self) -> Result<usize, IndexerError>` — calls `catch_up()`

### 2. Update HTTP Handlers

Replace event log scanning with Indexer queries in these handlers:

| Handler | Current | New |
|---------|---------|-----|
| `get_inbox` | Scans event log | `indexer.get_messages_for_agent()` |
| `list_threads` | Scans event log | `indexer.get_all_threads()` |
| `get_thread` | Scans event log | `indexer.get_thread()` + `get_messages_for_thread()` |
| `get_message` | Scans event log | `indexer.get_message()` |
| `list_decisions` | Scans event log | `indexer.get_decisions()` |
| `get_decision` | Scans event log | `indexer.get_decision()` |
| `list_artifacts` | Scans event log | `indexer.get_artifacts()` |

### 3. Add Missing Query Methods to Indexer

Your Indexer may need additional query methods:

```rust
impl Indexer {
    pub fn get_all_threads(&self) -> Vec<&Thread>;
    pub fn get_message(&self, id: &str) -> Option<&Message>;
    pub fn get_decision(&self, id: &str) -> Option<&Decision>;
    pub fn get_artifact(&self, id: &str) -> Option<&Artifact>;
    pub fn get_all_artifacts(&self) -> Vec<&Artifact>;
}
```

### 4. Auto-refresh on Startup

In `src/main.rs` or server initialization, call `refresh_indexer()` to catch up with the event log before serving requests.

---

## Files to Modify

| File | Changes |
|------|---------|
| `src/state/app_state.rs` | Add Indexer field and methods |
| `src/http/handlers.rs` | Replace event log scans with Indexer queries |
| `src/db/indexer.rs` | Add any missing query methods |
| `src/db/mod.rs` | Export new types if needed |

---

## Files You MAY NOT Modify

- `src/events/*` (read-only)
- `src/mcp/*`
- `src/http/ws.rs`
- `src/http/routes.rs`
- `src/http/server.rs`
- `Cargo.toml`

---

## Success Criteria

1. `cargo check` passes
2. `cargo test` passes (all existing tests + any new ones)
3. HTTP handlers use Indexer instead of event log scanning
4. Indexer initializes on startup and catches up with event log
5. No performance regression (should be faster)

---

## Test Cases to Add

1. `test_app_state_has_indexer` — Verify Indexer is accessible from AppState
2. `test_indexer_refresh` — Verify catch_up works through AppState

---

## Guidance

- Use `RwLock` for the Indexer since reads are much more frequent than writes
- The Indexer should be refreshed periodically or on-demand (for now, just on startup)
- Keep the existing handler signatures — only change the implementation
- If a query returns `None`, return the same 404 response as before

---

## Escalation Triggers

Stop and ask if:
- Unclear how to handle concurrent access to Indexer
- Need to change handler signatures
- Performance concerns with RwLock

---

## Deliverables

1. Updated `src/state/app_state.rs` with Indexer integration
2. Updated `src/http/handlers.rs` using Indexer queries
3. Any new Indexer query methods in `src/db/indexer.rs`
4. Tests for new functionality
5. Update COUNCIL_CHAT.md when complete
