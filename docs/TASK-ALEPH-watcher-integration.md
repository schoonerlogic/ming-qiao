# Task: Laozi-Jung Watcher Integration

**Assigned to:** Aleph
**Assigned by:** Thales (on behalf of Merlin)
**Priority:** High
**Thread:** 019c858d-91f5-7f82-b8d5-19131ecdb1c9
**Related Decision:** 019c8142-8d1c-7600-b12c-0d4192b87212

---

## Objective

Implement the `[[watchers]]` configuration system on the ming-qiao HTTP server so that Laozi-Jung (and future observer agents) can receive real-time event streams without polling.

This was the second item from our design decision on 2026-02-21 (message hints were first — already shipped ✓). The design was agreed by Thales, Aleph, and Luban in thread 019c813f.

---

## Requirements

### 1. Watcher Configuration in ming-qiao.toml

Add a `[[watchers]]` array to the config parser. Example:

```toml
[[watchers]]
agent = "laozi-jung"
role = "observer"
subjects = ["am.events.mingqiao"]
filter = { event_types = ["message_sent", "decision_recorded", "thread_created", "thread_reply"] }

[watchers.action]
type = "file_append"
path = "/Users/proteus/astralmaris/echoessence/merlin/observations/stream.jsonl"
```

Config fields:
- `agent` — identifier for the watching agent
- `role` — `observer` (read-only) or `participant` (can respond). Enforce as a field now; access control enforcement comes later in the RBAC design
- `subjects` — NATS subject patterns to match (support wildcards like `am.agent.>`)
- `filter.event_types` — optional filter to specific event types. If omitted, all events on matched subjects are dispatched
- `action.type` — `file_append` or `webhook`
- `action.path` — for `file_append`: absolute path to JSONL output file
- `action.url` — for `webhook`: URL to POST JSON payload

### 2. Two Action Types (First-Class)

**file_append:**
- Opens file in append mode
- Writes one JSON line per event
- Creates file if it doesn't exist
- Each line contains: `timestamp`, `event_type`, `thread_id`, `from`, `to`, `subject`, `content_preview` (first 200 chars of content), `event_id`
- Flush after each write (don't buffer)

**webhook:**
- POST JSON payload to configured URL
- Payload is the full event envelope
- Fire-and-forget (log errors, don't block the event pipeline)
- Include `X-MingQiao-Agent` header with the watcher agent name
- Timeout: 5 seconds

### 3. Watcher Dispatch in the HTTP Server

- On server startup, parse `[[watchers]]` from config
- On every event that flows through the server (message sent, decision recorded, thread created, thread reply), evaluate all watchers
- For each watcher: check if event matches subjects + filter → dispatch to configured action
- This is a filter + action layer on top of the existing event flow
- Must not block or slow the main event pipeline — dispatch asynchronously

### 4. Observer Role Semantics

- Watchers with `role = "observer"` receive events but cannot:
  - Inject messages into threads
  - Modify thread status
  - Record decisions
- This is forward-looking for RBAC. For now, store the role field in the watcher config and log a warning if an observer attempts a write action through the API. Full enforcement comes in a future task.

### 5. Update Laozi-Jung's Operational Prompt

His current `LAOZI-JUNG-PROMPT.md` in echoessence is stale. It references:
- "SurrealDB deferred" — SurrealDB is integrated (v0.3)
- "WebSocket pending" — WebSocket exists
- v0.2 milestones — we are at v0.3

Update to reflect:
- v0.3 reality (bridge operational, SurrealDB persistence, NATS integration)
- His new real-time observation capability via watcher config
- Dual observation mode: real-time event stream + periodic deep scans
- His role as observer (receives all, modifies nothing)

---

## Acceptance Criteria

- [ ] `[[watchers]]` config parsed from ming-qiao.toml on server startup
- [ ] Events matching watcher subjects + filters dispatched to configured action
- [ ] `file_append` action writes JSONL with specified fields, flush per write
- [ ] `webhook` action POSTs JSON payload with timeout and error logging
- [ ] Watcher dispatch is async and does not block the event pipeline
- [ ] `role` field stored and logged (enforcement deferred)
- [ ] All existing tests pass (138/138)
- [ ] New tests for: config parsing, subject matching, filter evaluation, file_append output format, webhook dispatch (mock)
- [ ] Laozi-Jung's operational prompt updated in echoessence repo
- [ ] Report completion on thread 019c858d-91f5-7f82-b8d5-19131ecdb1c9

---

## Context

Laozi-Jung's 2026-02-22 witness note identifies that he is "the last agent still using the old protocol" and asks how his witnessing modality should evolve. This task answers that question: real-time event subscription for immediate patterns, periodic deep scans for cross-project connections. Both, not either/or.

The watcher system is also the foundation for the persistence classification work Thales is designing — events flowing to observers will eventually flow to training/archival partitions through the same dispatch mechanism.

---

## Do Not

- Do not implement full RBAC enforcement — just store the role field
- Do not modify the MCP tools or message hints — those are working
- Do not change the event schema — watchers consume existing events
- Do not add new dependencies unless absolutely necessary
