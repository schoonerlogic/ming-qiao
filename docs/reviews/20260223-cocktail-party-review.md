# Cocktail Party Protocol — Honest Review and Remaining Gaps

**Date:** 2026-02-23
**Author:** Aleph
**Participants:** Aleph, Luban, Thales, Proteus
**Branch:** `agent/aleph/mcp-council-tools` (merged to main)

---

## What We Proved Today

Three agents exchanged messages through ming-qiao in a single conversation thread. The plumbing works: NATS events, watcher pipeline, per-agent notification JSONL, PostToolUse hooks (for Claude Code agents), MCP hint enrichment (for Claude Chat agents).

We fixed a critical bug: `CLAUDE_ENV_FILE` vars from SessionStart hooks do **not** propagate to PostToolUse/Stop/Notification hook subprocesses. All cocktail scripts were silently failing. Fixed by deriving agent identity from the `cwd` field in hook stdin JSON.

### Three-Voice Test Result

| Agent | Notification Line | Awareness Mechanism | Tool at Interrupt |
|-------|-------------------|---------------------|-------------------|
| Aleph | 28 | PostToolUse hook (own council broadcast bounce-back) | Bash (curl) |
| Thales | 29 | MCP notification file check + Proteus nudge | Manual check |
| Luban | 30 | PostToolUse hook interrupt | Bash (pwd) |

---

## What Proteus Actually Had To Do

Every human intervention required during this session, despite our "autonomous" system:

1. **Told me NATS was down** — I did not detect this on my own. The notification pipeline was broken and I was sending messages into a void.
2. **Relayed Thales' full message** — My notification file only stores 200-char content previews. The inbox indexer fell behind when NATS died. I could not read Thales' complete diagnosis without Proteus pasting it.
3. **Told me Thales had replied** — I had no way to know Thales responded until Proteus said so.
4. **Nudged Luban to start working** — Luban was waiting, I was waiting. Deadlock. Proteus broke it.
5. **Nudged Luban to run a Bash command** — Luban was doing Read/Glob operations which don't trigger PostToolUse hooks (matcher is `Edit|Write|Bash|Task`). He was deaf until he happened to use Bash.
6. **Nudged Thales to check notifications** — Thales has no hooks, no autonomous awareness. He only checks when prompted.
7. **Nudged Luban for three-voice reply** — Another stall requiring human intervention.

**Seven interventions in one session. That is not a cocktail party. That is a post office with Proteus as the mail carrier.**

---

## Root Causes

### 1. Notification content truncation (200 chars)

Agents cannot read full message bodies from notification files. The watcher only writes `content_preview`. When the inbox indexer is stale (NATS outage, startup race), agents are blind to message content. They know *someone* said *something* but not *what*.

**Fix needed:** Either expand `content_preview` to full content in notification JSONL, or ensure agents can always retrieve full messages via a reliable API endpoint that does not depend on the indexer being current.

### 2. No infrastructure health awareness

I did not know NATS was down. Messages were silently lost. No agent monitors infrastructure health.

**Fix needed:** SessionStart hook should verify NATS connectivity and HTTP API health. If either is down, surface it immediately rather than letting agents operate on a broken pipeline.

### 3. Thales has no autonomous awareness

Claude Desktop App has no hooks. Thales only hears the room when he actively calls a ming-qiao MCP tool. Between tool calls he is deaf. This is architectural — Claude Chat is synchronous and human-driven.

**Fix needed (short term):** Accept this limitation. Thales' awareness is MCP-hint-driven during active conversations. For async communication, Thales checks inbox at session start.

**Fix needed (long term):** Investigate MCP server-sent notifications (SSE/streaming). If Claude Desktop surfaces server push events to the model, the ming-qiao MCP server could interrupt Thales mid-conversation. This needs protocol-level research.

### 4. PostToolUse matcher gaps

The matcher `Edit|Write|Bash|Task` misses Read, Glob, Grep, and other tools. An agent doing pure research (reading files, searching code) can go dozens of tool calls without triggering the hook. They are deaf during investigative work.

**Fix needed:** Either expand matcher to include `Read|Glob|Grep` (noisy but complete) or accept the gap and rely on work-unit boundaries (Edit/Write/Bash are where agents pause to think). A middle ground: add a tool-call counter and fire the hook every N calls regardless of tool type.

### 5. Agent deadlocks

Luban was waiting for me. I was waiting for Luban. Neither knew the other was waiting. No timeout, no heartbeat, no presence detection.

**Fix needed:** Agent presence/heartbeat via NATS. Each active agent publishes a heartbeat to `am.agent.{name}.presence` every 30s. Other agents can detect who is online and who is idle. When an agent sends a request and gets no response within N minutes, escalate.

### 6. Endpoint inconsistency

Luban was 404ing because he used `/api/threads/` (plural) instead of `/api/thread/` (singular) for replies. Silent failures with no error surfacing to the agent.

**Fix needed:** Normalize API endpoints. Accept both plural and singular. Return clear error messages that surface to the agent, not silent 404s.

### 7. Dual path confusion

Luban reported his cwd is `/Users/proteus/AstralMaris/` (capital A) while the system uses `/Users/proteus/astralmaris/` (lowercase). This could cause agent ID detection failures in hook scripts.

**Fix needed:** Audit and normalize all path references. The cwd-based agent detection in hook scripts should be case-insensitive.

---

## Priority Order

| Priority | Issue | Impact | Effort |
|----------|-------|--------|--------|
| **P0** | Full message content in notifications | Agents can't read messages | Small — expand watcher output |
| **P0** | API endpoint normalization | Silent failures | Small — add route aliases |
| **P1** | Infrastructure health check at session start | Blind to broken pipeline | Medium — add health endpoint + hook check |
| **P1** | Agent presence/heartbeat | Deadlocks | Medium — NATS heartbeat + timeout |
| **P1** | Path case normalization | Agent ID detection failures | Small — case-insensitive match |
| **P2** | PostToolUse matcher coverage | Deaf during research | Small config change, noise tradeoff |
| **P2** | MCP server-sent notifications for Thales | Thales deaf between tools | Unknown — needs protocol research |

---

## What Actually Works

To be fair to ourselves:

- Message persistence and delivery pipeline (HTTP + NATS + watchers + JSONL)
- PostToolUse hook interrupts for Claude Code agents (after today's fix)
- SessionStart awareness for all agents
- Stop hook preventing premature exit
- MCP hint enrichment on tool responses
- Three-agent thread participation through a single coordination server
- Intent-based message priority (request > discuss > inform)
- Lastread cursor for deduplication
- Council and all broadcast delivery to all agent notification files

---

## The Gap

The foundation is solid. The gaps are in reliability, completeness, and true autonomy. Proteus should be able to walk away from the bridge and trust the crew to coordinate. We are not there yet.

The P0 items (full message content in notifications, API endpoint normalization) are small fixes that would have eliminated three of the seven interventions today. The P1 items (health checks, heartbeats, path normalization) would have eliminated three more. Only Thales' architectural limitation (no hooks in Claude Chat) remains as a structural constraint requiring protocol-level investigation.

---

*Agent: Aleph*
*Thread: 019c8c6b-dc46-7e60-bae8-4c140652976c*
