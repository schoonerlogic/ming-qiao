> **DEPRECATED (2026-02-22):** This file is archived. All agent coordination now flows through
> ming-qiao. Use the HTTP API (`GET /api/inbox/{agent}`, `GET /api/threads`) or MCP tools
> (`search_history`, `get_thread`) instead. See `ONBOARDING.md` for the new protocol.

# Agent Work Coordination — Ming-Qiao (ARCHIVED)

**Last Updated:** 2026-02-19T12:00:00Z
**Updated By:** aleph
**Status:** Archived — replaced by ming-qiao

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

- **Task:** SurrealDB Persistence Layer
- **Branch:** agent/luban/surrealdb-persistence
- **Status:** In progress — dependency approved, building SurrealDB schema
- **Signal received:** Aleph committed `messages.rs` with typed NATS message payloads
- **Message types to persist:** TaskAssignment, TaskStatusUpdate, SessionNote, Presence (24h TTL)
- **Dependency:** Uses `nats::messages` types for persistence schema
- **Design decisions confirmed (Thales/Aleph):**
  1. Persist Presence with 24h TTL (valuable for debugging/witnessing)
  2. Replace `db/indexer.rs` entirely (HashMap was placeholder)
  3. Remove JSONL layer (JetStream handles persistence)
  4. SurrealDB 3.0 with `kv-mem` + `rustls` features approved
- **Progress:**
  - ✅ Cargo.toml updated with surrealdb dependency
  - ✅ 123 tests passing, clean build
  - 🔄 Designing SurrealDB schema
- **Note:** Branched from `origin/agent/aleph/nats-bridge`, building against actual type definitions

### Previous Task (Complete)

- **Task:** Svelte UI Skeleton
- **Branch:** agent/luban/main/svelte-ui-skeleton
- **Status:** Complete — ready for review
- **Completed:** 7 components, 4 stores, API client, TypeScript types, SvelteKit + Tailwind setup
- **Note:** 31 files created (4071 lines), 0 TypeScript errors, 6 accessibility warnings

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
- Luban introduced as builder assistant (GLM-4.7 via Goose ACP in Zed Preview)
- Aleph runs in Zed (stable), Luban runs in Zed Preview (parallel agents)
- Coordination protocol defined in AGENTS.md
- Agent-specific instructions in agents/<name>/ directories
- Branch naming: agent/<agent>/<scope>/<task-description>

---

## Updates

### 2026-02-19T17:30:00Z — Luban Status Update

**Task:** SurrealDB Persistence Layer

**Branch:** `agent/luban/surrealdb-persistence`

**Status:** In progress — dependency approved, building SurrealDB schema

**Signal received:** Aleph committed `messages.rs` with typed NATS message payloads (Presence, TaskAssignment, TaskStatusUpdate, SessionNote)

**Design decisions confirmed (Thales/Aleph via Proteus):**
1. Persist Presence with 24h TTL (valuable for debugging/witnessing)
2. Replace `db/indexer.rs` entirely (HashMap was placeholder)
3. Remove JSONL layer (JetStream handles persistence)
4. SurrealDB 3.0 with `kv-mem` + `rustls` features approved

**Progress:**
- ✅ Cargo.toml updated with surrealdb dependency
- ✅ 123 tests passing, clean build
- 🔄 Designing SurrealDB schema

**Next:** Create SurrealDB tables matching NatsMessage types
