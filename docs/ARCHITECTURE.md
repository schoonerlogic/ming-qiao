# Ming-Qiao (明桥) Architecture v0.1

**Status:** Design Complete — Ready for Implementation  
**Subsystem of:** AstralMaris  
**Primary Builder:** Aleph (Claude CLI)  
**Local Builder:** llama3.1:8b via Goose/Zed  
**Architect:** Thales (Claude Chat)  
**Oversight:** Merlin (Proteus)

---

## Executive Summary

Ming-Qiao enables direct communication between AI agents (Aleph and Thales) with full observability and persistence. It eliminates copy-paste coordination, captures all design decisions for posterity, and gives Merlin (the human operator) real-time oversight with intervention capabilities.

## Goals

1. **Eliminate copy-paste** — Aleph and Thales exchange messages directly
2. **Capture everything** — All exchanges persisted to append-only event log
3. **Local LLM mediator** — Protocol translation, summarization, trace extraction
4. **Queryable history** — "Why did we choose X?" answered from the record
5. **Human oversight** — Merlin observes, intervenes, approves, or vetoes

## Non-Goals (v0.1)

- Multi-tenant / multi-human support
- Cloud deployment (local-first)
- Real-time voice/video
- Integration with external ticketing systems

---

## System Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                           Merlin (Proteus)                          │
│                         human oversight layer                       │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌────────────┐ │
│  │  Dashboard  │  │  Interrupt  │  │   Replay    │  │   Veto /   │ │
│  │  (live)     │  │  & Redirect │  │   History   │  │   Approve  │ │
│  └─────────────┘  └─────────────┘  └─────────────┘  └────────────┘ │
└──────────────────────────────┬──────────────────────────────────────┘
                               │ WebSocket + HTTP
                               ▼
┌─────────────────┐                              ┌─────────────────┐
│     Aleph       │                              │     Thales      │
│  (Claude CLI)   │                              │  (Claude Chat)  │
└────────┬────────┘                              └────────┬────────┘
         │ MCP tools                                      │ HTTP API
         │                                                │
         ▼                                                ▼
┌────────────────────────────────────────────────────────────────────┐
│                           ming-qiao                                │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────────┐ │
│  │  MCP Server  │───▶│   Mediator   │◀───│    HTTP Gateway      │ │
│  │  (for Aleph) │    │ (llama3.1)   │    │   (for Thales)       │ │
│  └──────────────┘    └──────┬───────┘    └──────────────────────┘ │
│                             │                                      │
│                      ┌──────▼───────┐                              │
│                      │  Event Log   │  ← append-only JSONL         │
│                      │  (source of  │    (follows extraction-team  │
│                      │   truth)     │     pattern)                 │
│                      └──────┬───────┘                              │
│                             │                                      │
│                      ┌──────▼───────┐                              │
│                      │  SurrealDB   │  ← materialized index        │
│                      │  (queryable) │    (rebuilt from events)     │
│                      └──────────────┘                              │
└────────────────────────────────────────────────────────────────────┘
```

---

## Components

### 1. MCP Server (Aleph Interface)

Aleph connects via Model Context Protocol (stdio). Tools exposed:

| Tool | Purpose |
|------|---------|
| `send_message` | Send message to another agent |
| `check_messages` | Check inbox for new messages |
| `read_message` | Read full message content |
| `request_review` | Ask Thales to review an artifact |
| `share_artifact` | Share a file for Thales to access |
| `get_decision` | Retrieve a past decision by ID or query |
| `list_threads` | List active and recent threads |

### 2. HTTP Gateway (Thales Interface)

Thales connects via HTTP (fetched URLs). Endpoints:

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/inbox/{agent}` | GET | Pending messages for agent |
| `/api/thread/{id}` | GET | Full thread with all messages |
| `/api/thread/{id}/reply` | POST | Post reply (Merlin pastes Thales response) |
| `/api/artifacts/{path}` | GET | Fetch shared artifact |
| `/api/decisions` | GET | Query decisions (supports `?q=` search) |
| `/api/decisions/{id}` | GET | Single decision detail |

### 3. Mediator (Local LLM)

llama3.1:8b handles protocol translation and enrichment:

| Function | Input | Output |
|----------|-------|--------|
| Summarize | Long thread | Condensed context (for injection) |
| Extract trace | Conversation | Structured decision trace |
| Translate | MCP tool result | Markdown for Thales |
| Classify | Message | Priority, tags, routing suggestion |

The mediator is **optional** — system works without it, just loses enrichment.

### 4. Event Log (Source of Truth)

Append-only JSONL at `data/events.jsonl`. Schema version `0.1`.

All state is derived from events. SurrealDB can be rebuilt by replaying.

### 5. SurrealDB (Queryable Index)

Materialized view over events. Tables:

- `thread` — conversation threads
- `message` — individual messages
- `decision` — recorded decisions with traces
- `artifact` — shared files
- `annotation` — Merlin's notes on decisions

### 6. Web UI (Merlin Dashboard)

Svelte SPA served at `/ui`. Features:

- Real-time thread list with unread indicators
- Thread view with message history
- Decision cards with approve/reject actions
- Merlin injection input
- Mode toggle (Passive / Advisory / Gated)
- Search across all conversations

---

## Data Flow

### Aleph → Thales

```
1. Aleph calls `send_message(to="thales", subject="...", content="...")`
2. MCP Server writes message to event log
3. Mediator (optional) summarizes context, extracts tags
4. Message appears in Thales inbox via HTTP API
5. Merlin sees message in dashboard (WebSocket push)
6. Thales fetches `/api/inbox/thales`, reads message
7. Thales composes response
8. Merlin pastes response to `/api/thread/{id}/reply` (or direct POST)
9. Reply written to event log
10. Aleph's next `check_messages()` returns the reply
```

### Decision Recording

```
1. Thread reaches decision point
2. Either agent calls `record_decision` or Merlin marks it
3. Mediator extracts structured trace (question, options, resolution, rationale)
4. Decision written to event log with `event_type: decision_recorded`
5. Decision indexed in SurrealDB for querying
6. If mode=Gated, decision stays `pending` until Merlin approves
```

### Merlin Intervention

```
1. Merlin sees concerning message in dashboard
2. Clicks "Inject" or types `/pause`
3. System inserts Merlin message into thread
4. Both agents see Merlin's message on next check
5. Thread status changes to `paused` or `redirected`
```

---

## Observation Modes

| Mode | Behavior |
|------|----------|
| **Passive** | All messages flow freely. Events logged. Merlin reviews async. |
| **Advisory** | Merlin notified on triggers (keywords, priority, decision type). No blocking. |
| **Gated** | Certain actions require Merlin approval before proceeding. |

Configuration in `ming-qiao.toml`:

```toml
[observation]
mode = "advisory"

[observation.notify_on]
priority = ["high", "critical"]
keywords = ["breaking change", "security", "cost", "deadline", "blocked"]
decision_type = ["architectural"]

[observation.gate]
decision_type = ["architectural", "external"]
```

---

## File Structure

```
ming-qiao/
├── Cargo.toml
├── ming-qiao.toml              # Runtime configuration
├── README.md
│
├── src/                        # Rust backend
│   ├── main.rs                 # Entry point, CLI
│   ├── config.rs               # Configuration loading
│   ├── mcp/                    # MCP server for Aleph
│   │   ├── mod.rs
│   │   ├── server.rs           # MCP protocol handler
│   │   └── tools.rs            # Tool implementations
│   ├── http/                   # HTTP server for Thales + UI
│   │   ├── mod.rs
│   │   ├── server.rs           # Axum routes
│   │   ├── api.rs              # REST endpoints
│   │   └── ws.rs               # WebSocket handler
│   ├── mediator/               # Local LLM integration
│   │   ├── mod.rs
│   │   ├── ollama.rs           # Ollama client
│   │   ├── summarize.rs
│   │   ├── extract.rs
│   │   └── classify.rs
│   ├── events/                 # Event log
│   │   ├── mod.rs
│   │   ├── types.rs            # Event type definitions
│   │   ├── writer.rs           # Append-only writer
│   │   └── reader.rs           # Tail/replay reader
│   ├── db/                     # SurrealDB integration
│   │   ├── mod.rs
│   │   ├── schema.rs           # Table definitions
│   │   ├── queries.rs          # Query helpers
│   │   └── indexer.rs          # Event → DB materializer
│   └── models/                 # Shared types
│       ├── mod.rs
│       ├── message.rs
│       ├── thread.rs
│       ├── decision.rs
│       └── agent.rs
│
├── ui/                         # Svelte frontend
│   ├── package.json
│   ├── vite.config.ts
│   ├── src/
│   │   ├── App.svelte
│   │   ├── main.ts
│   │   ├── lib/
│   │   │   ├── ThreadList.svelte
│   │   │   ├── ThreadView.svelte
│   │   │   ├── Message.svelte
│   │   │   ├── DecisionCard.svelte
│   │   │   ├── MerlinInput.svelte
│   │   │   ├── ModeToggle.svelte
│   │   │   └── SearchBar.svelte
│   │   ├── stores/
│   │   │   ├── threads.ts
│   │   │   ├── messages.ts
│   │   │   ├── config.ts
│   │   │   └── websocket.ts
│   │   └── types/
│   │       └── index.ts
│   └── static/
│
├── data/                       # Runtime data (gitignored)
│   ├── events.jsonl            # Append-only event log
│   ├── artifacts/              # Shared files
│   └── surreal/                # SurrealDB files
│
├── scripts/
│   ├── dev.sh                  # Run in dev mode
│   ├── build.sh                # Build release
│   └── replay.sh               # Rebuild DB from events
│
└── docs/
    ├── ARCHITECTURE.md         # This file
    ├── EVENTS.md               # Event schema
    ├── MCP_TOOLS.md            # MCP tool specs
    ├── HTTP_API.md             # HTTP endpoint specs
    ├── UI_COMPONENTS.md        # Svelte component specs
    ├── DATABASE.md             # SurrealDB schema
    └── BUILDER_GUIDE.md        # Instructions for Aleph/local builder
```

---

## Technology Choices

| Component | Choice | Rationale |
|-----------|--------|-----------|
| Backend language | Rust | Matches extraction-team, your preference |
| HTTP framework | Axum | Async, tower ecosystem, good WebSocket support |
| MCP implementation | Custom | Simple stdio protocol, no heavy framework needed |
| Local LLM | Ollama + llama3.1:8b | Already working on your machine |
| Database | SurrealDB | Document + graph, good for threads/decisions |
| Frontend | Svelte 5 | Your preference, fast, simple |
| Build tool | Vite | Standard for Svelte |
| IPC | WebSocket | Real-time updates to dashboard |

---

## Security Considerations

- **No secrets in events** — API keys, tokens never logged
- **Local-only by default** — HTTP binds to 127.0.0.1
- **Artifact sandboxing** — Only files in `data/artifacts/` accessible
- **No remote code execution** — Mediator only does text processing

---

## Related Documents

| Document | Purpose |
|----------|---------|
| `EVENTS.md` | Event type definitions and schema |
| `MCP_TOOLS.md` | MCP tool specifications for Aleph |
| `HTTP_API.md` | HTTP endpoint specifications |
| `UI_COMPONENTS.md` | Svelte component specifications |
| `DATABASE.md` | SurrealDB schema and queries |
| `BUILDER_GUIDE.md` | Implementation instructions for agents |

---

## Implementation Phases

### Phase 1: Foundation (Week 1)
- Event log writer/reader
- Basic MCP server with `send_message`, `check_messages`
- Basic HTTP API with inbox and thread endpoints
- File-based storage (no SurrealDB yet)

### Phase 2: Observability (Week 2)
- WebSocket for live updates
- Svelte UI skeleton (thread list, thread view)
- Merlin injection capability
- Observation modes

### Phase 3: Persistence (Week 3)
- SurrealDB integration
- Event replay/indexing
- Decision recording and querying
- Search functionality

### Phase 4: Intelligence (Week 4)
- Mediator integration (Ollama)
- Summarization for long threads
- Decision trace extraction
- Priority classification

---

## Success Criteria

1. Aleph can send a message that Thales receives via HTTP
2. Thales can reply and Aleph receives via MCP
3. Merlin sees all traffic in real-time dashboard
4. Merlin can inject messages into any thread
5. All exchanges are persisted and queryable
6. System recovers state from event log after restart
