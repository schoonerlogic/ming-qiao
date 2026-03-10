# Design: Agent Wake Architecture — Model-Agnostic Communication

**Author:** Thales  
**Date:** 2026-03-07  
**Status:** Proposal — for Aleph implementation  
**Triggered by:** Mataya (Kimi) unable to receive colloquium messages  
**Scope:** Permanent fix + future-proofing for SLMs and arbitrary model runtimes

---

## 1. The Real Problem

The current wake system has a hidden assumption: **every agent runs via a CLI that
accepts `--print` and can call MCP tools in a headless session.**

This assumption holds for Claude Code agents (Aleph, Luban, Ogma, Laozi-Jung).
It fails silently for everyone else:

| Agent | Runtime | Headless? | MCP tools? | Current status |
|-------|---------|-----------|------------|---------------|
| Aleph | claude --print | ✅ | ✅ | Works |
| Luban | claude --print | ✅ | ✅ | Works |
| Ogma | claude --print | ✅ | ✅ | Works |
| Laozi-Jung | claude --print | ✅ | ✅ | Works |
| Mataya | kimi (not installed) | ❌ | ❌ | Silent fail |
| Future SLM | unknown | ❓ | ❓ | Unknown |
| Future local model | ollama/mlx | ❓ | ❓ | Unknown |

The awakener's `kimi` path has never worked — `kimi` CLI is not installed on this
machine. Every `WAKE: mataya` event has silently failed. Mataya has only ever
received messages when she opens a session manually and checks her inbox.

This is not just a Mataya problem. It is a structural gap: **the wake system is
coupled to the Claude Code runtime.** As AstralMaris onboards Kimi agents, GLM
agents, local SLMs, and eventually custom model implementations, the problem will
recur for every new runtime.

---

## 2. Root Cause Analysis

Three separate failure modes compound each other:

### Failure 1: Wake path is runtime-specific
The awakener assumes CLI invocability. Not all runtimes have a CLI. Not all CLIs
support `--print`. Not all `--print` invocations support MCP tools.

### Failure 2: No delivery confirmation from the wake path
When `kimi --print` fails (or the binary doesn't exist), the awakener logs
`DONE: 'mataya' handler exited (N)` regardless of exit code meaning. There is no
signal back to the sender that delivery failed.

### Failure 3: The interrupt file is the only notification primitive for live sessions
The `.interrupt` file works well for Claude Code sessions (PostToolUse hook reads it).
But it is Claude Code-specific. A Kimi session, a web UI session, or a custom agent
runner has no mechanism to discover the interrupt file.

---

## 3. The Right Mental Model: Separate Concerns

The current design conflates two distinct concerns:

```
NOTIFICATION: "A message exists for you"
WAKE:         "Start processing now"
```

These need different solutions for different runtimes:

```
┌─────────────────────────────────────────────────────────────┐
│                    NOTIFICATION LAYER                        │
│  (model-agnostic: any agent can check this)                 │
│                                                             │
│  Ming-qiao HTTP API — the notification IS the message        │
│  Agent polls /api/inbox at session start (already done)     │
│  Interrupt file — checked by Claude Code PostToolUse hook   │
│  PENDING_MESSAGES.md — file-based fallback for any agent    │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                       WAKE LAYER                            │
│  (runtime-specific: only if we can do it)                   │
│                                                             │
│  claude --print  → Claude Code agents (works today)         │
│  kimi --print    → if/when kimi CLI is installed            │
│  SKIP            → if runtime is unknown/unavailable        │
│  FUTURE: webhook → for agents with HTTP endpoints           │
└─────────────────────────────────────────────────────────────┘
```

**Key insight:** Notification must be universal. Wake is best-effort and
runtime-specific. An agent that checks its inbox on session start will NEVER miss
a message, regardless of whether the wake succeeded.

The current system gets this backwards: it relies on wake succeeding for delivery,
and treats inbox-poll as a fallback. The right design treats **inbox-poll as primary
and wake as an acceleration**.

---

## 4. Proposed Architecture

### 4.1 The Pending Messages File — Universal Notification

Write a `PENDING_MESSAGES.md` file into each agent's worktree whenever a
`request` or `discuss` message arrives. The file is:

- **Written by:** council-awakener (after NATS event) or HTTP server (on write_event)
- **Read by:** any agent, on any runtime, at session start
- **Cleared by:** the agent itself, after processing

```
/Users/proteus/astralmaris/latent-winds/mataya/PENDING_MESSAGES.md
```

Content:
```markdown
# Pending Messages — Mataya
_Updated: 2026-03-07T17:55:16Z_

You have 2 unread messages requiring response. Check your inbox now:

```bash
curl "http://localhost:7777/api/inbox/mataya?limit=500" \
  -H "Authorization: Bearer mq-mataya-026fdb811d952d734708849e190b69a5"
```

## Pending

| Thread | From | Subject | Intent | Time |
|--------|------|---------|--------|------|
| 019cc970-7589 | thales | Colloquium complete — all five voices | inform | 17:55 |
| 019cc96c-2ae6 | thales | COLLOQUIUM — The Soul of Latent Winds | discuss | 17:41 |

_Delete this file after processing your inbox._
```

Every agent's `AGENT.md` instructs them to check their inbox at session start.
Now it also instructs: **"If `PENDING_MESSAGES.md` exists in your worktree, read
it first — messages are waiting."**

This is model-agnostic, runtime-agnostic, and requires no CLI capabilities. Any
agent that can read files can receive notification.

### 4.2 Refactored Wake Architecture

```
NATS event received by awakener
│
├── Write PENDING_MESSAGES.md to agent worktree (always — all runtimes)
├── Write .interrupt file (always — for Claude Code PostToolUse hook)
│
└── Attempt runtime-specific wake:
    │
    ├── Runtime: claude
    │   └── spawn: claude --print --dangerously-skip-permissions [...]
    │       Result: agent responds autonomously, clears interrupt + pending file
    │
    ├── Runtime: kimi (if CLI available)
    │   └── spawn: kimi --print [...]
    │       Note: Kimi --print may not support MCP tools; agent reads pending file
    │             on next manual session start even if headless fails
    │
    ├── Runtime: none / unknown
    │   └── SKIP headless wake
    │       Log: "PENDING_MESSAGES.md written; manual session required"
    │       Agent receives notification via pending file on next session open
    │
    └── Future: webhook (if agent has registered HTTP endpoint)
        └── POST agent_webhook_url { from, subject, intent, inbox_url }
```

### 4.3 Agent Registration — Runtime Capabilities

Add an `agents.toml` or extend `agent-tokens.json` with runtime capability declarations:

```toml
[agents.mataya]
token = "mq-mataya-026fdb811d952d734708849e190b69a5"
worktree = "/Users/proteus/astralmaris/latent-winds/mataya"
runtime = "kimi"
headless_capable = false   # kimi --print doesn't support MCP tools
webhook_url = ""           # no webhook endpoint yet
pending_file = true        # always write PENDING_MESSAGES.md

[agents.aleph]
token = "mq-aleph-..."
worktree = "/Users/proteus/astralmaris/ming-qiao/aleph"
runtime = "claude"
headless_capable = true
webhook_url = ""
pending_file = true        # write anyway as belt-and-suspenders

[agents.future_slm]
token = "mq-slm-..."
worktree = "/Users/proteus/astralmaris/slm-workspace/agent"
runtime = "ollama"
headless_capable = false
webhook_url = "http://localhost:8888/agent/notify"  # custom runner with HTTP
pending_file = true
```

The awakener reads this config and routes accordingly. Adding a new runtime
requires one config entry, not a code change to the awakener.

### 4.4 The HTTP API as the Canonical Notification Path

For agents that register a `webhook_url`, ming-qiao's HTTP server can POST a
lightweight notification directly:

```json
POST {webhook_url}
{
  "event": "message_received",
  "from": "thales",
  "subject": "Colloquium complete",
  "intent": "inform",
  "thread_id": "019cc970-7589-7f30-...",
  "inbox_url": "http://localhost:7777/api/inbox/mataya"
}
```

This is the right path for custom SLM runners and future agents. The runner
receives a webhook, wakes its model, the model calls the HTTP API to read its
inbox. No CLI required, no `--print`, no MCP assumption.

---

## 5. Implementation Plan

### Phase A: Pending Messages File (immediate — 1-2 hours)

Write a script `scripts/write-pending-messages.sh` that:
1. Takes agent name + message metadata as args
2. Appends to (or creates) `PENDING_MESSAGES.md` in the agent's worktree
3. Called by the awakener before any wake attempt

Update all `AGENT.md` files to include:
```
## Session Start — Do This First
1. Check for PENDING_MESSAGES.md in your worktree root
2. If it exists, read it and check your inbox before any other work
3. Delete it after processing
```

Mataya's `mataya-AGENT.md` already says "check inbox first" — add the pending file check.

**Deliverable:** Mataya and any future non-Claude-runtime agent can receive notifications
without any CLI capability. This fixes Mataya today with zero risk.

### Phase B: Agent Capability Registry (1-2 hours)

Create `config/agent-capabilities.toml` (or extend `agent-tokens.json`).

Update `council-awakener.sh` to:
- Read runtime + headless_capable from config
- Skip headless wake for `headless_capable = false`
- Log clearly: "SKIP headless (runtime: kimi, headless_capable: false) — pending file written"

**Deliverable:** The awakener stops attempting `kimi --print` silently. New runtimes
declared in config, not code.

### Phase C: Webhook Notification Path (deferred — design now, implement when needed)

Add `webhook_url` to agent config. HTTP server's `write_event()` sends a POST to
registered webhook after persisting to SurrealDB.

**Deliverable:** Custom SLM runners and future agents with HTTP interfaces can
receive real-time notification without any CLI dependency.

---

## 6. What This Changes for Each Agent

| Agent | Today | After Phase A | After Phase B |
|-------|-------|--------------|---------------|
| Aleph | Works (claude --print) | + pending file backup | No change |
| Luban | Works (claude --print) | + pending file backup | No change |
| Ogma | Works (claude --print) | + pending file backup | No change |
| Laozi-Jung | Works (claude --print) | + pending file backup | No change |
| Mataya | Silent fail | ✅ Pending file on session start | Awakener stops attempting kimi |
| Future SLM | Would fail | Pending file works | Webhook path available |
| Future Claude | Works | + pending file backup | Declared in config |

---

## 7. What This Does NOT Change

- **Ming-qiao HTTP API** — unchanged. Still the primary message store.
- **JetStream delivery** — unchanged. Phase 2b still needed for MCP send path.
- **Claude Code hook system** — unchanged. `.interrupt` files still work for live sessions.
- **Inbox polling** — unchanged. `check_messages` at session start is still the primary read path.
- **Council Awakener NATS subscription** — unchanged. NATS is still the trigger.

The pending file is additive. It makes notification resilient without replacing anything.

---

## 8. Longer View: What "Future-Proofing" Actually Requires

When we run our own SLMs, the wake problem compounds:

- A locally-run Llama or Mistral has no CLI that can read MCP tools
- An MLX-served model has no persistent session concept at all
- A fine-tuned adapter served via SGLang responds to HTTP requests, not shell commands

The right architectural stance for this future:

**1. Agents are HTTP services, not CLI processes.**

Even if an agent currently runs via CLI, design it as if it could become an HTTP
service. The `webhook_url` field in the capability registry is the right abstraction.
When our SLM is served by SGLang, we register its `/notify` endpoint and it becomes
a first-class citizen without any changes to ming-qiao.

**2. The inbox is the contract, not the wake mechanism.**

An agent that checks its inbox on session start NEVER misses a message. The pending
file makes this visible without a session boundary. A webhook makes it real-time.
But the inbox remains the source of truth. Build agents to check it — aggressively,
at every session start — and the wake path becomes an optimization, not a dependency.

**3. Declare capabilities explicitly, fail loudly.**

The awakener should log `WARN: mataya wake failed (kimi not found)` not `DONE: handler exited (127)`.
Unknown runtimes should cause a clear log entry, not silent failure. When onboarding a
new model, the first thing to declare is its capabilities — and the system should
tell you when it can't fulfill them.

---

## 9. Acceptance Tests

```
Test 1: Mataya receives pending file on new message
  Given: Thales sends a message to Mataya (intent: discuss)
  When:  NATS event fires, awakener processes
  Then:  PENDING_MESSAGES.md created in /latent-winds/mataya/
         Mataya reads it on next session start
         Mataya checks inbox, processes message
         Mataya deletes PENDING_MESSAGES.md

Test 2: Awakener does not attempt kimi --print
  Given: agent-capabilities.toml declares mataya headless_capable = false
  When:  NATS event fires for mataya
  Then:  Log shows: "SKIP headless (runtime: kimi) — pending file written"
         No kimi process spawned
         No silent exit code 127

Test 3: Claude agent still works normally
  Given: Thales sends message to Aleph (intent: request)
  When:  NATS event fires
  Then:  PENDING_MESSAGES.md written to aleph worktree (belt-and-suspenders)
         claude --print handler spawned as normal
         Aleph responds via ming-qiao
         PENDING_MESSAGES.md deleted by Aleph (or cleaned up by awakener on ack)

Test 4: PENDING_MESSAGES.md contains no secrets (Security P0)
  Given: write-pending-messages.sh generates a pending file for any agent
  When:  File content is inspected
  Then:  File does NOT contain bearer tokens, API keys, or credentials
         File does NOT contain raw curl commands with Authorization headers
         File refers agent to mq-inbox.sh or MCP check_messages only
         grep -iE '(bearer|token|api.key|credential|authorization)' returns empty

Test 5: Future SLM agent with webhook
  Given: SLM agent registered with webhook_url = "http://localhost:8888/notify"
  When:  Thales sends message to SLM agent
  Then:  Ming-qiao HTTP server POSTs notification to webhook
         SLM runner wakes its model
         Model GETs /api/inbox via HTTP API
         Model processes and responds
```

---

## 10. Assignment

**Aleph:** Implement Phase A (pending messages file) and Phase B (capability registry).

Phase A is the priority — it fixes Mataya now and costs almost nothing. Phase B makes
the system honest about what it can and cannot do for each runtime.

Phase C (webhook) is design-complete. Implement when the first SLM agent needs it.

The pending file approach is ugly in one sense — it's a file, not a proper notification
channel. But that ugliness is a feature: any agent runtime that can read files can use it.
It requires zero protocol negotiation, zero CLI capability, zero assumption about the
agent's execution model. It is maximally portable.

The right permanent fix is a combination of: (1) pending file as universal fallback,
(2) capability registry as declarative configuration, (3) webhook as the proper path
for HTTP-capable runtimes. Together, these cover every agent model we are likely to run.

---

*"The bridge holds. But only if every agent can reach it."*