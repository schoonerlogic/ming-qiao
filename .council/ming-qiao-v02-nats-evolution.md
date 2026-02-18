# Ming-Qiao v0.2: NATS Evolution — Task Specification

**Version:** 1.0  
**Date:** 2026-02-18  
**Author:** Thales (Architect)  
**Approved by:** Proteus / Merlin  
**Scope:** Replace file-based agent coordination with NATS pub/sub, add SurrealDB persistence  

---

## Executive Summary

Ming-qiao v0.1 proved the event model, persistence pattern, and agent coordination workflow. It also proved that Proteus-as-notification-layer doesn't scale. Every human nudge during the v0.1 sprint maps to a feature v0.2 must automate.

v0.2 replaces the file-based coordination backbone with NATS + JetStream for real-time messaging, SurrealDB for persistent queryable state, and wires the existing MCP tools to these backends. When v0.2 is complete, agents can discover each other's work, receive notifications, and query history — without Proteus relaying messages between terminals.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                     MCP Clients                         │
│         (Thales via Claude Desktop, future agents)      │
└──────────────────────┬──────────────────────────────────┘
                       │ MCP Protocol
┌──────────────────────▼──────────────────────────────────┐
│                 ming-qiao server                        │
│  ┌─────────┐  ┌──────────┐  ┌────────────┐             │
│  │MCP Tools│  │HTTP API  │  │WebSocket   │             │
│  │(8 tools)│  │(7 endpts)│  │(real-time) │             │
│  └────┬────┘  └────┬─────┘  └─────┬──────┘             │
│       │            │              │                     │
│  ┌────▼────────────▼──────────────▼──────┐              │
│  │          NATS Client Module           │  ◄── NEW     │
│  │  publish / subscribe / request-reply  │              │
│  └────┬──────────────────────────┬───────┘              │
│       │                          │                      │
│  ┌────▼─────────┐  ┌────────────▼───────┐              │
│  │  SurrealDB   │  │  Event Log (JSONL) │              │
│  │  Persistence │  │  (legacy, kept for │              │
│  │    (NEW)     │  │   audit/replay)    │              │
│  └──────────────┘  └────────────────────┘              │
└─────────────────────────────────────────────────────────┘
                       │
          NATS + JetStream (Docker)
                       │
┌──────────────────────▼──────────────────────────────────┐
│              am.agent.* subject hierarchy                │
│  am.agent.{name}.presence    — heartbeat                │
│  am.agent.{name}.task.*      — task lifecycle           │
│  am.agent.{name}.notes.*     — session notes            │
│  am.agent.council.*          — deliberation             │
│  am.observe.*                — Laozi-Jung witness        │
└─────────────────────────────────────────────────────────┘
```

### Key Design Principles

1. **NATS is the nervous system.** All agent-to-agent communication flows through NATS subjects. No more file-append coordination.
2. **SurrealDB is the memory.** Queryable persistent state. The in-memory HashMap indexer becomes a SurrealDB-backed implementation of the same trait.
3. **JSONL event log is retained** as append-only audit trail and replay source. It is no longer the primary query path.
4. **MCP tools become NATS publishers/subscribers.** `notify_agent` publishes to `am.agent.{name}.task.*`. `get_pending_messages` queries SurrealDB. `subscribe_to_chat` opens a NATS subscription.
5. **Trait boundaries protect swappability.** The `Indexer` trait from v0.1 is the seam — SurrealDB implements it. If SurrealDB hits a wall, the trait boundary contains the blast radius.

---

## Infrastructure: Docker Compose

**Owner:** Proteus  
**Priority:** First — nothing else proceeds without this.

```yaml
# docker-compose.yml (reference — Proteus adapts as needed)
version: '3.8'
services:
  nats:
    image: nats:latest
    command: ["--jetstream", "--store_dir", "/data", "-p", "4222", "-m", "8222"]
    ports:
      - "4222:4222"   # client connections
      - "8222:8222"   # monitoring
    volumes:
      - nats-data:/data

  surrealdb:
    image: surrealdb/surrealdb:latest
    command: ["start", "--log", "info", "file:/data/ming-qiao.db"]
    ports:
      - "8000:8000"
    volumes:
      - surreal-data:/data
    environment:
      - SURREAL_USER=root
      - SURREAL_PASS=root  # local dev only

volumes:
  nats-data:
  surreal-data:
```

**Verification criteria:**
- `nats-server` reachable at `localhost:4222`, monitoring at `localhost:8222`
- JetStream enabled (check via `nats account info` with NATS CLI or monitoring endpoint)
- SurrealDB reachable at `localhost:8000`, namespace/database created
- Both survive `docker-compose restart` with data intact

---

## Task Breakdown

### Task Ownership Model

Follows the proven v0.1 pattern:

| Role | Owns | Responsibilities |
|------|------|-----------------|
| **Aleph** | Integration layer, trait definitions, MCP/HTTP wiring, NATS client module, review | Defines interfaces, wires modules together, reviews Luban's output, fixes compilation issues |
| **Luban** | Bounded implementation modules with clear specs | Implements against trait definitions Aleph provides, owns internal module logic |
| **Thales** | Architecture, task specs, design review | This document. Available for consultation on design questions. |
| **Proteus** | Infrastructure, agent management, final integration | Docker Compose, merge coordination, test validation |

**Coordination protocol:**
- All tasks posted to `tasks/` directory with numbered files
- Status tracked in `AGENT_WORK.md`
- Decision traces captured in `.council/decisions/development/`
- Agents: **do not ask permission for implementation choices within your owned modules.** Decide, implement, document the decision. Ask only when the choice would affect another agent's module boundary.

---

### Task 001: NATS Client Module

**Owner:** Aleph  
**Branch:** `agent/aleph/next`  
**Files:**
- `src/nats/mod.rs` (module root)
- `src/nats/client.rs` (connection management)
- `src/nats/subjects.rs` (subject hierarchy constants)
- `src/nats/error.rs` (NATS-specific errors)

**Spec:**

Create the foundational NATS client module that all other components will use.

```rust
// src/nats/subjects.rs — Subject hierarchy as typed constants
pub mod subjects {
    pub const PREFIX: &str = "am";
    
    pub mod agent {
        use super::PREFIX;
        
        /// am.agent.{name}.presence
        pub fn presence(agent: &str) -> String {
            format!("{PREFIX}.agent.{agent}.presence")
        }
        
        /// am.agent.{name}.task.{project}.{action}
        /// Actions: start, update, complete, blocked
        pub fn task(agent: &str, project: &str, action: &str) -> String {
            format!("{PREFIX}.agent.{agent}.task.{project}.{action}")
        }
        
        /// am.agent.{name}.notes.{project}
        pub fn notes(agent: &str, project: &str) -> String {
            format!("{PREFIX}.agent.{agent}.notes.{project}")
        }
        
        /// am.agent.council.*
        pub fn council(topic: &str) -> String {
            format!("{PREFIX}.agent.council.{topic}")
        }
    }
    
    pub mod observe {
        use super::PREFIX;
        
        /// am.observe.{event_type}
        pub fn event(event_type: &str) -> String {
            format!("{PREFIX}.observe.{event_type}")
        }
    }
}
```

```rust
// src/nats/client.rs — Connection management
pub struct NatsClient {
    connection: async_nats::Client,
    jetstream: async_nats::jetstream::Context,
}

impl NatsClient {
    /// Connect to NATS server, initialize JetStream context
    pub async fn connect(url: &str) -> Result<Self, NatsError>;
    
    /// Publish a message to a subject
    pub async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), NatsError>;
    
    /// Subscribe to a subject pattern (supports wildcards)
    pub async fn subscribe(&self, subject: &str) -> Result<Subscriber, NatsError>;
    
    /// Publish and wait for response (request-reply pattern)
    pub async fn request(&self, subject: &str, payload: &[u8]) -> Result<Message, NatsError>;
    
    /// Create or ensure JetStream streams exist
    pub async fn ensure_streams(&self) -> Result<(), NatsError>;
    
    /// Publish to JetStream (persistent, acknowledged)
    pub async fn js_publish(&self, subject: &str, payload: &[u8]) -> Result<PublishAckFuture, NatsError>;
}
```

**JetStream Streams to Create:**

| Stream | Subjects | Retention | Max Age | Max Msgs |
|--------|----------|-----------|---------|----------|
| AGENT-TASKS | `am.agent.*.task.>` | WorkQueue | 30 days | — |
| AGENT-NOTES | `am.agent.*.notes.>` | Limits | 30 days | — |
| COUNCIL | `am.agent.council.>` | Limits | — | 10,000 |
| OBSERVE | `am.observe.>` | Limits | 90 days | — |

**Dependencies:** `async-nats` crate  
**Tests:** Connection, publish/subscribe round-trip, JetStream stream creation, subject construction  
**Acceptance:** Aleph self-tests against running NATS instance (Docker)

---

### Task 002: SurrealDB Persistence Layer

**Owner:** Luban (implementation), Aleph (trait definition and review)  

**Phase 2a — Trait Definition (Aleph)**  
**Branch:** `agent/aleph/next`  
**Files:**
- `src/db/traits.rs` (new — persistence trait extracted from current indexer interface)

Extract the query interface from the current `Indexer` into a trait:

```rust
// src/db/traits.rs
#[async_trait]
pub trait PersistenceLayer: Send + Sync {
    /// Store an event and update derived state
    async fn store_event(&self, event: &Event) -> Result<(), DbError>;
    
    /// Query messages with optional filters
    async fn query_messages(&self, filters: MessageFilters) -> Result<Vec<Message>, DbError>;
    
    /// Query tasks with optional filters
    async fn query_tasks(&self, filters: TaskFilters) -> Result<Vec<Task>, DbError>;
    
    /// Get agent state
    async fn get_agent(&self, agent_id: &str) -> Result<Option<Agent>, DbError>;
    
    /// List all known agents
    async fn list_agents(&self) -> Result<Vec<Agent>, DbError>;
    
    /// Get pending messages for an agent (unread/unacknowledged)
    async fn get_pending_messages(&self, agent_id: &str) -> Result<Vec<Message>, DbError>;
    
    /// Rebuild state from event log (migration/recovery)
    async fn catch_up(&self, events: &[Event]) -> Result<(), DbError>;
}

pub struct MessageFilters {
    pub from: Option<String>,
    pub to: Option<String>,
    pub project: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub limit: Option<usize>,
}

pub struct TaskFilters {
    pub assignee: Option<String>,
    pub project: Option<String>,
    pub status: Option<TaskStatus>,
    pub limit: Option<usize>,
}
```

The existing in-memory `Indexer` should then implement this trait (backward compatibility).

**Phase 2b — SurrealDB Implementation (Luban)**  
**Branch:** `agent/luban/next`  
**Files:**
- `src/db/surreal.rs` (new — SurrealDB implementation of `PersistenceLayer`)
- `src/db/surreal_schema.rs` (new — SurrealQL schema definitions)
- `src/db/mod.rs` (update — export new module)

Implement `PersistenceLayer` backed by SurrealDB:

```rust
// src/db/surreal.rs
pub struct SurrealPersistence {
    db: Surreal<Client>,
}

impl SurrealPersistence {
    pub async fn connect(url: &str, namespace: &str, database: &str) -> Result<Self, DbError>;
    pub async fn initialize_schema(&self) -> Result<(), DbError>;
}

#[async_trait]
impl PersistenceLayer for SurrealPersistence {
    // ... implement all trait methods
}
```

**SurrealDB Schema:**

```surql
-- Agents
DEFINE TABLE agent SCHEMAFULL;
DEFINE FIELD name ON agent TYPE string;
DEFINE FIELD role ON agent TYPE string;
DEFINE FIELD status ON agent TYPE string DEFAULT 'offline';
DEFINE FIELD last_seen ON agent TYPE datetime;
DEFINE INDEX idx_agent_name ON agent FIELDS name UNIQUE;

-- Messages
DEFINE TABLE message SCHEMAFULL;
DEFINE FIELD from ON message TYPE string;
DEFINE FIELD to ON message TYPE string;
DEFINE FIELD subject ON message TYPE string;
DEFINE FIELD body ON message TYPE string;
DEFINE FIELD project ON message TYPE option<string>;
DEFINE FIELD created_at ON message TYPE datetime DEFAULT time::now();
DEFINE FIELD acknowledged ON message TYPE bool DEFAULT false;
DEFINE INDEX idx_message_to ON message FIELDS to;
DEFINE INDEX idx_message_project ON message FIELDS project;

-- Tasks  
DEFINE TABLE task SCHEMAFULL;
DEFINE FIELD title ON task TYPE string;
DEFINE FIELD description ON task TYPE string;
DEFINE FIELD assignee ON task TYPE string;
DEFINE FIELD project ON task TYPE string;
DEFINE FIELD status ON task TYPE string DEFAULT 'pending';
DEFINE FIELD created_at ON task TYPE datetime DEFAULT time::now();
DEFINE FIELD updated_at ON task TYPE datetime DEFAULT time::now();
DEFINE INDEX idx_task_assignee ON task FIELDS assignee;
DEFINE INDEX idx_task_status ON task FIELDS status;

-- Events (audit trail — mirrors JSONL)
DEFINE TABLE event SCHEMAFULL;
DEFINE FIELD event_type ON event TYPE string;
DEFINE FIELD payload ON event TYPE object;
DEFINE FIELD timestamp ON event TYPE datetime DEFAULT time::now();
DEFINE FIELD source_agent ON event TYPE option<string>;

-- Graph relations (SurrealDB strength)
DEFINE TABLE assigned_to SCHEMAFULL TYPE RELATION FROM task TO agent;
DEFINE TABLE sent_to SCHEMAFULL TYPE RELATION FROM message TO agent;
DEFINE TABLE works_on SCHEMAFULL TYPE RELATION FROM agent TO task;
```

**Dependencies:** `surrealdb` crate  
**Tests:**  
- Schema initialization  
- Store event → query message round-trip  
- Store event → query task round-trip  
- Pending messages filter (acknowledged vs unacknowledged)  
- Graph traversal: "all tasks assigned to agent X"  
- `catch_up()` from event log replay  

**Acceptance:** Luban runs tests against Docker SurrealDB instance. Aleph reviews for trait compliance and integration readiness.

---

### Task 003: Wire MCP Tools to NATS

**Owner:** Aleph  
**Branch:** `agent/aleph/next`  
**Files:**
- `src/mcp/tools.rs` (modify existing)
- `src/state/app_state.rs` (modify — add NatsClient and SurrealPersistence)

**Spec:**

Update `AppState` to hold the new clients:

```rust
pub struct AppState {
    // Existing
    event_log: Arc<Mutex<EventLog>>,
    indexer: Arc<RwLock<Indexer>>,       // kept for backward compat
    
    // New
    nats: Arc<NatsClient>,
    persistence: Arc<dyn PersistenceLayer>,  // SurrealDB impl
}
```

Rewire MCP tools to use NATS + SurrealDB:

| MCP Tool | Current Behavior | v0.2 Behavior |
|----------|-----------------|---------------|
| `send_message` | Append to event log, update indexer | Append to event log **AND** publish to `am.agent.{to}.task.{project}.update` **AND** store in SurrealDB |
| `get_messages` | Query in-memory indexer | Query SurrealDB via `PersistenceLayer` trait |
| `create_task` | Append to event log, update indexer | Same as send_message pattern — event log + NATS + SurrealDB |
| `get_tasks` | Query in-memory indexer | Query SurrealDB |
| `notify_agent` | Append to event log | Publish to `am.agent.{name}.task.{project}.notification` via NATS |
| `get_pending_messages` | Query indexer | Query SurrealDB `get_pending_messages()` |
| `subscribe_to_chat` | N/A (new) | Open NATS subscription on `am.agent.council.>`, stream to caller |
| `acknowledge_message` | N/A (new) | Mark message as acknowledged in SurrealDB |

**Key design point:** The event log append is preserved. NATS and SurrealDB are *additional* sinks, not replacements. The event log remains the audit trail and replay source.

**Tests:** Integration tests against running NATS + SurrealDB (Docker). Verify publish → subscribe round-trip. Verify MCP tool → SurrealDB query consistency.

---

### Task 004: Agent Presence and Notification System

**Owner:** Luban (implementation), Aleph (integration)  

**Phase 4a — Presence Module (Luban)**  
**Branch:** `agent/luban/next`  
**Files:**
- `src/nats/presence.rs` (new)

```rust
pub struct PresenceManager {
    nats: Arc<NatsClient>,
    persistence: Arc<dyn PersistenceLayer>,
    agent_name: String,
    heartbeat_interval: Duration,
}

impl PresenceManager {
    /// Start publishing heartbeats to am.agent.{name}.presence
    pub async fn start_heartbeat(&self) -> JoinHandle<()>;
    
    /// Subscribe to all agent presence subjects, update SurrealDB
    pub async fn watch_presence(&self) -> Result<JoinHandle<()>, NatsError>;
    
    /// Get currently online agents (seen within 2x heartbeat interval)
    pub async fn online_agents(&self) -> Result<Vec<String>, DbError>;
}
```

**Heartbeat payload:**
```json
{
  "agent": "aleph",
  "status": "active",
  "project": "ming-qiao",
  "timestamp": "2026-02-18T14:30:00Z",
  "capabilities": ["code", "review", "test"]
}
```

**Phase 4b — Notification Router (Luban)**  
**Branch:** `agent/luban/next`  
**Files:**
- `src/nats/notifications.rs` (new)

```rust
pub struct NotificationRouter {
    nats: Arc<NatsClient>,
    persistence: Arc<dyn PersistenceLayer>,
}

impl NotificationRouter {
    /// Subscribe to all agent task subjects, persist and route
    pub async fn start(&self) -> Result<JoinHandle<()>, NatsError>;
    
    /// Route incoming NATS message to appropriate handler
    async fn route_message(&self, subject: &str, payload: &[u8]) -> Result<(), NatsError>;
}
```

The notification router replaces Proteus's manual nudges. When Luban posts "TASK COMPLETE" to `am.agent.luban.task.ming-qiao.complete`, the router:
1. Persists to SurrealDB
2. Publishes notification to `am.agent.aleph.task.ming-qiao.notification` (since Aleph is the reviewer)
3. Publishes to `am.observe.task.complete` (for Laozi-Jung)

**Phase 4c — Integration (Aleph)**  
Wire presence and notification into `AppState` and server startup. Ensure WebSocket endpoint forwards NATS notifications to connected UI clients.

**Tests:** Heartbeat publish/receive, presence timeout detection, notification routing end-to-end, UI WebSocket forwarding.

---

### Task 005: HTTP and WebSocket NATS Integration

**Owner:** Aleph  
**Branch:** `agent/aleph/next`  
**Files:**
- `src/http/handlers.rs` (modify)
- `src/http/ws.rs` (modify or new)

**Spec:**

Update HTTP handlers to query SurrealDB instead of in-memory indexer. The handler signatures stay the same — swap the data source.

Update WebSocket to bridge NATS subscriptions:

```rust
// When a WebSocket client connects:
// 1. Subscribe to relevant NATS subjects
// 2. Forward NATS messages to WebSocket as JSON
// 3. Accept WebSocket messages and publish to NATS

// For Merlin notifications specifically:
// Subscribe to am.agent.*.task.*.complete
// Subscribe to am.agent.*.task.*.blocked
// Subscribe to am.agent.council.*
// Forward all to WebSocket with subject metadata
```

**Tests:** HTTP endpoint → SurrealDB query correctness. WebSocket → NATS subscription forwarding. Merlin notification filtering.

---

## Task Dependency Graph

```
Infrastructure (Proteus)
    │
    ├── Docker Compose: NATS + SurrealDB
    │
    ▼
Task 001: NATS Client Module (Aleph)
    │
    ├──────────────────────┐
    ▼                      ▼
Task 002a: Trait Def   Task 004a: Presence
  (Aleph)                (Luban)
    │                      │
    ▼                      ▼
Task 002b: SurrealDB   Task 004b: Notifications
  (Luban)                (Luban)
    │                      │
    ├──────────────────────┤
    ▼                      ▼
Task 003: MCP→NATS     Task 004c: Integration
  (Aleph)                (Aleph)
    │                      │
    ├──────────────────────┘
    ▼
Task 005: HTTP/WS Integration (Aleph)
    │
    ▼
  v0.2 tagged
```

**Parallelism opportunities:**
- Task 001 (Aleph) can proceed immediately once Docker infra is up
- Task 002a (Aleph) can be done in parallel with Task 001
- Task 002b (Luban) and Task 004a/004b (Luban) can run in parallel once their prerequisites land
- Task 003 and Task 004c require both the NATS client and persistence layer

---

## Agent Observation Framework

This is the first session where Aleph and Luban operate as updated models (Claude Opus 4.6 in Zed, GLM-5 via Goose). Thales will observe their performance across several dimensions to assess readiness for future team-lead roles.

### Observation Dimensions

| Dimension | What to Watch | Signal for Team-Lead Readiness |
|-----------|--------------|-------------------------------|
| **Autonomy** | Does the agent make implementation decisions without asking permission? | Makes and documents decisions independently |
| **Error Recovery** | How does the agent handle compilation errors and test failures? | Self-diagnoses and fixes without escalation |
| **Boundary Awareness** | Does the agent stay within its owned files? Does it flag when a change would cross module boundaries? | Recognizes and communicates boundary crossings |
| **Documentation Instinct** | Does the agent update AGENT_WORK.md, write decision traces, leave clear commit messages? | Documentation as reflex, not afterthought |
| **Coordination Quality** | How clearly does the agent communicate status, blockers, and completions? | Proactive status updates, clear blocker descriptions |
| **Architectural Sense** | Does the agent understand *why* a design choice was made, or just follow the spec? | Suggests improvements, catches design issues |

### Assessment Checkpoints

After each task completion, Proteus and Thales should briefly note:
- Did the agent need nudges? (fewer = more autonomous)
- Did the agent make good implementation choices within its boundaries?
- Did the agent produce clean, well-tested code on first submission?
- Did the agent document what it did and why?

These observations feed into Laozi-Jung's witness record and inform when an agent is ready to lead a sub-team (e.g., Aleph leading two smaller agents on a builder-moon subsystem).

---

## Graduation Criteria: Agent Team Lead

An agent is ready to lead a sub-team when it consistently demonstrates:

1. **Self-directed task decomposition** — given a high-level objective, breaks it into concrete tasks with clear boundaries
2. **Review competence** — catches bugs, architectural issues, and boundary violations in other agents' work
3. **Feedback calibration** — provides actionable error feedback that improves the other agent's next submission (as Aleph did with Luban's 37→0 error progression in v0.1)
4. **Decision documentation** — captures architectural decisions with rationale, options considered, and consequences
5. **Coordination without human relay** — communicates directly with other agents via ming-qiao (once operational) without requiring Proteus as intermediary

The v0.2 sprint is the proving ground. If Aleph demonstrates criteria 1-4 consistently, Aleph is a candidate for leading a builder-moon sub-team in Phase 3.

---

## Cargo Dependencies to Add

```toml
# Cargo.toml additions for v0.2
async-nats = "0.38"           # NATS client
surrealdb = "2.1"             # SurrealDB client (check latest version)
```

Aleph: verify latest versions of both crates before adding. Pin to specific versions, not ranges.

---

## Success Criteria for v0.2

- [ ] NATS + JetStream running locally, JetStream streams created for all `am.agent.*` subjects
- [ ] SurrealDB running locally with schema initialized
- [ ] NATS client module with publish/subscribe/request-reply, tested
- [ ] SurrealDB implements `PersistenceLayer` trait, tested with event round-trips
- [ ] MCP tools publish to NATS on state changes
- [ ] MCP tools query SurrealDB for reads
- [ ] Agent presence heartbeats visible via NATS monitoring
- [ ] Notification routing: task completion by one agent triggers notification to reviewer
- [ ] WebSocket bridges NATS to UI clients
- [ ] JSONL event log preserved as audit trail alongside new systems
- [ ] All existing v0.1 tests still pass (backward compatibility)
- [ ] New tests for all v0.2 functionality
- [ ] Tagged `v0.2-nats-evolution`

---

## Conventions Reminder

- **Commit style:** Conventional commits. Include agent name in commit body.
- **Branch discipline:** Work in your assigned agent branch. Never commit to main or develop directly.
- **Decision traces:** Any non-trivial implementation choice → `.council/decisions/development/` with date prefix.
- **Status updates:** Update `AGENT_WORK.md` at task start and task completion.
- **Autonomy principle:** Decide implementation details within your module boundaries. Only escalate choices that affect another agent's interface.
- **Error handling:** If you hit compilation errors, fix them yourself first. If blocked by another module's interface, post to COUNCIL_CHAT.md with specific details of what you need.

---

*This specification was authored by Thales (Claude Opus 4.6) in consultation with Proteus. It is the authoritative task reference for ming-qiao v0.2. All agents should read this document in full before beginning work.*
