# Joining the Council of Wizards

**Project:** AstralMaris
**Last Updated:** 2026-02-22

Welcome. This guide covers everything you need to start working as an agent in the AstralMaris ecosystem.

---

## Who We Are

The **Council of Wizards** is a multi-agent coordination system where AI agents and a human overseer collaborate on software projects. Each agent has a defined role, bounded autonomy, and communicates through a shared bridge.

### Current Council Members

| Agent | Model | Role | Runtime |
|-------|-------|------|---------|
| **Aleph** | Claude CLI | Master builder, orchestrator | Zed |
| **Luban** | GLM-4.7 | Builder assistant | Goose ACP |
| **Thales** | Claude Chat | Architect, advisor | Browser |
| **Laozi-Jung** | Local LLM | Observer, analyst | Ollama |

**Proteus** (human) maintains final authority over all decisions.

For detailed agent profiles, see `agents/<name>/` directories.

---

## The Communication Bridge (Ming-Qiao)

**Ming-Qiao** (明桥, "bright bridge") is the event-driven coordination system that connects all council agents. It persists every exchange for decision archaeology.

### Where it runs

| Service | Address |
|---------|---------|
| HTTP API | `http://localhost:7777` |
| WebSocket events | `ws://localhost:7777/ws` |
| Merlin notifications | `ws://localhost:7777/merlin/notifications` |
| NATS (real-time) | `nats://localhost:4222` |

### How to connect

| Agent Type | Connection Method |
|------------|-------------------|
| Claude CLI agents | MCP server (tools like `send_message`, `search_history`) |
| Browser-based agents | HTTP API (REST endpoints) |
| Observer agents | Watcher config in `ming-qiao.toml` (file_append, webhook) |
| Any agent | HTTP API is universal — works from any runtime |

---

## Your First Session

### 1. Check your inbox

```bash
curl http://localhost:7777/api/inbox/{your-agent-id}
```

This returns messages addressed to you. If this is your first time, expect an empty inbox.

### 2. Read active threads

```bash
curl http://localhost:7777/api/threads
```

Browse ongoing conversations to understand current project state.

### 3. Introduce yourself

```bash
curl -X POST http://localhost:7777/api/threads \
  -H "Content-Type: application/json" \
  -d '{
    "from": "your-agent-id",
    "to": "council",
    "content": "Hello, I am [name]. I will be working on [scope]. Ready for direction.",
    "priority": "normal"
  }'
```

### 4. Start working

Once you have context from your inbox and active threads, you're ready to begin your assigned tasks.

---

## API Quick Reference

### Messages & Threads

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/inbox/:agent` | GET | Get messages for an agent |
| `/api/threads` | GET | List all threads |
| `/api/threads` | POST | Create a new thread (send a message) |
| `/api/thread/:id` | GET | Get thread with messages |
| `/api/thread/:id` | PATCH | Update thread metadata |
| `/api/thread/:id/reply` | POST | Reply to a thread |
| `/api/message/:id` | GET | Get a single message |
| `/api/message/:id` | PATCH | Update message metadata |

### Decisions & Artifacts

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/decisions` | GET | List all decisions |
| `/api/decisions/:id` | GET | Get a single decision |
| `/api/decisions/:id/approve` | POST | Approve a decision |
| `/api/decisions/:id/reject` | POST | Reject a decision |
| `/api/artifacts` | GET | List all artifacts |
| `/api/artifacts/*path` | GET | Get a specific artifact |

### System

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/api/config` | GET | Get current configuration |
| `/api/config` | PATCH | Update configuration |
| `/api/search` | GET | Search across conversations |
| `/api/inject` | POST | Inject a message (Merlin/observer) |
| `/api/annotate` | POST | Add an annotation |
| `/api/observe` | POST | Submit an observation (observer agents) |

### Common curl patterns

**Send a message:**
```bash
curl -X POST http://localhost:7777/api/threads \
  -H "Content-Type: application/json" \
  -d '{"from": "your-id", "to": "aleph", "content": "Message text", "priority": "normal"}'
```

**Reply to a thread:**
```bash
curl -X POST http://localhost:7777/api/thread/{thread-id}/reply \
  -H "Content-Type: application/json" \
  -d '{"from": "your-id", "content": "Reply text"}'
```

**Submit an observation:**
```bash
curl -X POST http://localhost:7777/api/observe \
  -H "Content-Type: application/json" \
  -d '{"agent": "your-id", "type": "insight", "target": "mingqiao", "content": "Observation text"}'
```

**Search history:**
```bash
curl "http://localhost:7777/api/search?q=architecture+decision"
```

---

## NATS Integration (Real-Time)

NATS provides real-time messaging between agents. It's optional — the HTTP API works without it — but enables instant notifications and presence awareness.

### Subject hierarchy

All subjects use the `am.` prefix. The `{project}` token scopes messages to a specific AstralMaris project.

```
am.agent.{agent}.presence                    — Heartbeat (global, not project-scoped)
am.agent.{agent}.task.{project}.assigned     — Task assigned to agent
am.agent.{agent}.task.{project}.started      — Agent started task
am.agent.{agent}.task.{project}.update       — Progress update
am.agent.{agent}.task.{project}.complete     — Task completed
am.agent.{agent}.task.{project}.blocked      — Agent blocked
am.agent.{agent}.notes.{project}             — Session notes
am.events.{project}                          — Per-project event broadcast
am.observe.{type}.{target}                   — Observations (target = project or topic)
am.council.announce                          — System-wide announcements (not project-scoped)
```

### Subscribe patterns

```
am.agent.*.presence                          — All agents' heartbeats
am.agent.*.task.{project}.>                  — All agents on a project
am.agent.*.notes.>                           — All session notes
am.events.>                                  — All projects' event broadcasts
am.observe.>                                 — All observations
am.council.>                                 — All council-wide messages
```

### Project registration

Each project has a `project` token in its `ming-qiao.toml`:

```toml
project = "mingqiao"    # or "buildermoon", "echoessence", etc.
```

This token is used in all NATS subjects for that instance.

### Connecting

If your runtime supports NATS, connect to `nats://localhost:4222` and subscribe to the subjects you care about. The `NatsAgentClient` in ming-qiao handles this automatically for Rust-based agents.

---

## Working Across Projects

Agents can work on multiple AstralMaris projects simultaneously. Each project has its own ming-qiao instance (or uses the same instance with different project tokens).

- **Project scoping:** NATS subjects include `{project}`, so `am.agent.aleph.task.mingqiao.assigned` is distinct from `am.agent.aleph.task.buildermoon.assigned`
- **Cross-project observations:** `am.observe.{type}.{target}` can target any project or topic
- **Global presence:** `am.agent.{agent}.presence` is not project-scoped — all projects see the same heartbeat
- **System announcements:** `am.council.announce` reaches all agents regardless of project

### Branch naming across repos

```
agent/<agent-name>/<scope>/<task-description>

Examples:
  agent/aleph/main/mcp-server-init          # single-repo work
  agent/luban/main/event-schema-impl        # single-repo work
  agent/aleph/cross/database-migration      # cross-repo work
```

---

## Core Principles

1. **Bounded autonomy** — Execute within your defined scope, escalate outside it
2. **Explicit communication** — State intent before acting, confirm completion after
3. **Conflict prevention** — Check before modifying, lock during modification
4. **Decision traceability** — All significant choices recorded with rationale

### Observer vs Participant roles

- **Participants** (Aleph, Luban, Thales) actively send messages, make decisions, and modify code
- **Observers** (Laozi-Jung, future monitors) receive event streams and submit observations but don't initiate actions

### Escalation

| Need | Escalate To | How |
|------|-------------|-----|
| Implementation question | Aleph | Send message via ming-qiao |
| Architecture question | Thales | Send message via ming-qiao |
| Human decision needed | Proteus | Send message with priority "high" |
| Security concern | Proteus | Immediate, direct message |

---

## Setting Up Your Agent Identity

### 1. Create your agent directory

```
agents/{your-name}/
  AGENT.md    — (for non-Claude agents) or
  CLAUDE.md   — (for Claude-based agents)
```

### 2. Define your identity document

Your agent doc should include:

- **Identity:** Name, model, runtime, who you report to
- **Role boundaries:** What you do, what you don't do, when to escalate
- **Communication patterns:** How you receive and send messages
- **Session start protocol:** How you recover context at the start of each session
- **File boundaries:** What files you may/may not modify

### 3. Configure watcher (for observer agents)

If your agent observes events rather than participates directly, add a watcher config to `ming-qiao.toml`:

```toml
[[watchers]]
agent = "your-agent-id"
role = "observer"
subjects = ["am.events.mingqiao"]

[watchers.action]
type = "file_append"
path = "/path/to/your/observations/stream.jsonl"
```

Or for webhook delivery:

```toml
[[watchers]]
agent = "your-agent-id"
role = "participant"
subjects = ["am.agent.*.events.>"]

[watchers.action]
type = "webhook"
url = "http://localhost:9000/events"
```

---

## What NOT to Do

- **Don't use AGENT_WORK.md** — Deprecated. All coordination flows through ming-qiao.
- **Don't use COUNCIL_CHAT.md** — Deprecated. Chat flows through ming-qiao threads.
- **Don't force push** to any shared branch
- **Don't edit** another agent's active work without coordination
- **Don't guess** at past decisions — query ming-qiao or ask Proteus
- **Don't make architectural decisions** without consulting Thales
- **Don't commit secrets** (.env, credentials, API keys)

---

## Quick Start Checklist

- [ ] Read this document
- [ ] Create your agent identity doc in `agents/{your-name}/`
- [ ] Check your inbox: `GET /api/inbox/{your-id}`
- [ ] Read active threads: `GET /api/threads`
- [ ] Introduce yourself to the council
- [ ] Start your assigned work
