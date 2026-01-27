# Decision Trace: Indexer Integration Sprint

**Date:** 2026-01-25
**Participants:** Aleph (builder), Luban (implementation), Thales (advisory)
**Scope:** Tasks 004-005, MCP/HTTP integration testing

---

## Decision 1: In-Memory Indexer Before SurrealDB

**Context:** Need queryable state from append-only event log. Full SurrealDB integration would add complexity.

**Options:**
1. Implement SurrealDB integration immediately
2. Start with in-memory HashMap, add SurrealDB later
3. Skip indexer, scan event log on every query

**Decision:** Option 2 — In-memory HashMap first

**Rationale:**
- Faster iteration cycle for core functionality
- Proves the event→model transformation logic
- SurrealDB can be swapped in later without changing query interface
- O(1) lookups vs O(n) event log scans

**Consequences:**
- State lost on restart (acceptable for v0.1)
- Must add `catch_up()` on startup to rebuild from event log
- Query methods return references, not owned data (clone before releasing lock)

---

## Decision 2: Luban Task Boundaries

**Context:** Delegating indexer work to Luban while maintaining architectural coherence.

**Options:**
1. Give Luban full ownership of db/ and http/handlers.rs
2. Split: Luban owns db/, Aleph owns integration points
3. Aleph implements everything directly

**Decision:** Option 2 — Split ownership with clear boundaries

**Rationale:**
- Luban excels at bounded implementation tasks with clear specs
- Aleph maintains integration coherence (AppState wiring, HTTP handler signatures)
- Reduces coordination overhead by limiting file conflicts

**Task Assignments:**
- Task 004 (Luban): `src/db/indexer.rs`, `src/db/state.rs`, `src/db/error.rs`
- Task 005 (Luban): Add to Task 004 files + `src/state/app_state.rs`, `src/http/handlers.rs`
- Aleph: Review, fix compilation issues, test integration

**Consequences:**
- Clear ownership prevents merge conflicts
- Luban can work independently once spec is clear
- Aleph must review thoroughly before merging

---

## Decision 3: Error Resolution Approach

**Context:** Luban's Task 004 had 37 compilation errors on first submission.

**Options:**
1. Fix errors directly in Luban's code
2. Provide detailed error list, let Luban fix
3. Reject task, ask for complete rewrite

**Decision:** Option 2 — Detailed feedback, Luban fixes

**Rationale:**
- Preserves Luban's ownership and learning
- Specific error messages more useful than vague rejection
- Aleph's time better spent on integration than implementation fixes

**Error Categories Identified:**
- EventPayload variant names (MessageSent → Message)
- Type mismatches (UUID vs String)
- Model field names (title vs subject, sent_at vs created_at)
- Iterator type handling (replay vs after return different types)

**Consequences:**
- Luban successfully fixed all 37 errors
- Established feedback pattern for future tasks
- Task 005 had zero compilation errors (learning applied)

---

## Decision 4: AppState Indexer Initialization

**Context:** How to initialize Indexer in AppState when event log may not exist.

**Options:**
1. Fail if event log missing
2. Create empty indexer, fail on first query
3. Create empty indexer, create empty event log file
4. Lazy initialization on first access

**Decision:** Option 3 — Create empty indexer and file

**Rationale:**
- Server should start even with no history
- Empty event log is valid state (new installation)
- Avoids Option/Result complexity in accessor methods
- `refresh_indexer()` handles catch-up when events exist

**Implementation:**
```rust
if !events_path.exists() {
    // Create parent dirs and empty file
    std::fs::create_dir_all(parent)?;
    std::fs::File::create(&events_path)?;
}
let indexer = Indexer::new(&events_path)?;
```

**Consequences:**
- Clean startup experience
- No special handling needed in handlers
- Indexer starts empty, catches up on `refresh_indexer()`

---

## Decision 5: Handler Concurrency Pattern

**Context:** HTTP handlers need to read from shared Indexer without blocking writes.

**Options:**
1. Clone entire indexer on each request
2. Hold read lock while serializing JSON
3. Acquire lock, clone needed data, release lock, then serialize
4. Use channels for request/response

**Decision:** Option 3 — Clone data, release lock, then serialize

**Rationale:**
- Minimizes lock hold time
- JSON serialization can be slow, shouldn't block other readers
- Clone overhead acceptable for expected query volume
- Simple to implement and reason about

**Pattern:**
```rust
let indexer = state.indexer().await;
let data = indexer.get_foo().cloned().collect::<Vec<_>>();
drop(indexer);  // Release lock
Json(serde_json::json!({ "items": data }))
```

**Consequences:**
- High read concurrency
- Small memory overhead from cloning
- Clear separation of data access and serialization

---

## Decision 6: Filter Logic Fix

**Context:** During review, found inverted filter logic in handlers.

**Original (incorrect):**
```rust
.filter(|msg| {
    if let Some(ref from) = query.from {
        &msg.from != from  // WRONG: excludes matches
    } else { true }
})
```

**Decision:** Fix immediately rather than file issue

**Rationale:**
- Trivial fix (change `!=` to `==`)
- Bug would cause incorrect API responses
- No architectural implications

**Consequences:**
- Filters now correctly include matching items
- No additional review cycle needed

---

## Summary

| Decision | Choice | Risk Level |
|----------|--------|------------|
| In-memory before SurrealDB | Incremental | Low |
| Split task ownership | Coordination | Medium |
| Detailed error feedback | Process | Low |
| Create empty event log | Graceful startup | Low |
| Clone-then-serialize | Performance | Low |
| Immediate filter fix | Correctness | Low |

**Total Tests:** 80 passing
**Both Servers:** Operational
**Next:** SurrealDB integration or additional MCP tools
