# Agent Work Coordination — Ming-Qiao

**Last Updated:** 2026-02-19T12:00:00Z
**Updated By:** aleph

---

## Active Work

### Aleph

- **Task:** NATS Agent Client — purpose-specific coordination bus
- **Branch:** agent/aleph/nats-bridge
- **Status:** Complete — all 4 modules implemented and pushed
- **Completed:**
  - `src/nats/subjects.rs` — Subject hierarchy builder (AgentSubjects struct)
  - `src/nats/messages.rs` — Typed NATS payloads (Presence, TaskAssignment, TaskStatusUpdate, SessionNote)
  - `src/nats/streams.rs` — JetStream stream/consumer configs (AGENT_TASKS, AGENT_NOTES)
  - `src/nats/client.rs` — NatsAgentClient with typed publish/subscribe API
  - Simplified NatsConfig (removed subject/stream fields, now code-defined)
  - Refactored bridge.rs → client.rs (purpose-specific channels replace V1 firehose)
- **Note:** 123 tests passing, clean compile, branch pushed to origin

### Luban

- **Task:** SurrealDB persistence (pending assignment)
- **Branch:** TBD
- **Status:** Available — NATS message types pushed for schema reference
- **Dependency:** Uses `nats::messages` types (TaskAssignment, TaskStatusUpdate, SessionNote) for persistence schema

### Thales

- **Task:** Agent Client Design review
- **Status:** Available (advisory role)
- **Notes:** Designed the purpose-specific channel architecture (presence/tasks/notes). Aleph implemented Option B (no unified firehose).

---

## Completed (This Sprint)

- [x] **Aleph:** V1 NATS bridge (connect, publish EventEnvelope, subscribe)
- [x] **Aleph:** Integration test — full round-trip MCP → NATS → HTTP
- [x] **Aleph:** subjects.rs — AgentSubjects struct with method-based subject hierarchy
- [x] **Aleph:** messages.rs — 4 typed message structs + NatsMessage tagged enum
- [x] **Aleph:** streams.rs — AGENT_TASKS + AGENT_NOTES streams, 4 consumer configs
- [x] **Aleph:** client.rs — NatsAgentClient refactor (replaces NatsBridge)
- [x] **Aleph:** Simplified NatsConfig (enabled + url only)
- [x] **Aleph:** Moved EventType Display impl to events/schema.rs

## Previous

- [x] Aleph: Merlin notification system (MerlinNotifier, WebSocket endpoint)
- [x] Luban: Svelte UI Skeleton (7 components, 4 stores, API client)
- [x] Luban: Indexer Integration (7 HTTP handlers using O(1) lookups)
- [x] Luban: Database Indexer (10 tests, all 6 event types)
- [x] Luban: Event Persistence Layer (EventWriter, EventReader)
- [x] Luban: Database Models (6 models, 3 enums)
- [x] Luban: Event Schema Foundation (14 tests)
- [x] Aleph: MCP server (8 tools), HTTP gateway (7 endpoints), WebSocket

---

## Blocked / Waiting

_No active blockers._

---

## Upcoming

- [ ] Assign Luban: SurrealDB persistence layer (uses NATS message types for schema)
- [ ] Wire NATS subscriptions into server startup (start consumers, broadcast received messages)
- [ ] Add presence heartbeat timer (periodic publish)
- [ ] Wire typed NATS publishing into MCP tools (task assignment → publish_task_assignment)
- [ ] Review Luban's Svelte UI implementation
- [ ] Wire up 10 stub HTTP handlers to EventWriter

## System Status

**Components Operational:**

- ✅ Event persistence (JSONL append-only log)
- ✅ Database indexer (in-memory materialized views)
- ✅ MCP server (8 tools for Aleph)
- ✅ HTTP gateway (7 endpoints for Thales)
- ✅ WebSocket event stream (`/ws`)
- ✅ Merlin notification system (`/merlin/notifications`)
- ✅ Observation modes (Passive/Advisory/Gated)
- ✅ NATS agent client (typed publish/subscribe, graceful degradation)

**Test Status:** 123/123 passing

**NATS Module Structure:**

```
src/nats/
├── mod.rs       — Module re-exports
├── subjects.rs  — AgentSubjects: am.agent.{agent}.{channel}.{project}.*
├── messages.rs  — Presence, TaskAssignment, TaskStatusUpdate, SessionNote
├── streams.rs   — AGENT_TASKS (work queue), AGENT_NOTES (30-day)
└── client.rs    — NatsAgentClient: connect, publish, subscribe
```

**Servers:**

- HTTP: `http://localhost:7777`
- WebSocket events: `ws://localhost:7777/ws`
- Merlin notifications: `ws://localhost:7777/merlin/notifications`
- NATS: `nats://localhost:4222` (requires `nats-server --jetstream`)

---

## Communication Log

| Timestamp        | From   | To     | Summary                                      |
| ---------------- | ------ | ------ | -------------------------------------------- |
| 2026-02-18       | Aleph  | Thales | NATS bridge V1 complete, integration verified |
| 2026-02-19       | Thales | Aleph  | Agent Client Design: purpose-specific channels |
| 2026-02-19       | Aleph  | Thales | Chose Option B, implemented 4 modules         |
| 2026-02-19       | Aleph  | Luban  | Messages pushed — ready for SurrealDB schema  |

---

## Notes

- NATS integration uses graceful degradation: disabled by default, `connect()` returns None if unreachable
- Subject hierarchy: `am.agent.{agent}.presence`, `am.agent.{agent}.task.{project}.*`, `am.agent.{agent}.notes.{project}`
- JetStream streams: AGENT_TASKS (work queue, 7 days), AGENT_NOTES (limits, 30 days)
- Presence uses core NATS (ephemeral, no persistence)
- Agent convention: agents push branches, Proteus merges to develop from merlin worktree
