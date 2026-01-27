# Session Retrospective: First Aleph-Luban Coordination

**Date:** 2026-01-25  
**Duration:** ~4 hours  
**Project:** ming-qiao  
**Participants:** Proteus (human), Aleph (Claude CLI), Luban (GLM-4.7/Goose), Thales (Claude Chat)

---

## What We Built

### AstralMaris Agent Kit v0.2
- Deployed to ming-qiao and extraction-team
- `--merge` mode for existing projects
- Decision trace infrastructure (`.council/`)
- Agent instruction templates with runtime-specific naming

### Ming-Qiao Implementation
- Tasks 001-005 completed
- 80 tests passing
- MCP server: 8 tools operational
- HTTP server: 7 endpoints with Indexer integration
- In-memory indexer (SurrealDB deferred)

---

## What Worked

| Success | Evidence |
|---------|----------|
| Clear task boundaries | No file conflicts across 5 tasks |
| AGENT_WORK.md status tracking | Clean progression visible |
| Decision traces | Aleph captured 6 decisions with full rationale |
| Error feedback loop | Luban fixed 37 errors after detailed feedback, Task 005 had zero |
| Split ownership model | Luban owns db/, Aleph owns integration points |

---

## What Needed Human Intervention

| Friction | Frequency | Workaround |
|----------|-----------|------------|
| "Check COUNCIL_CHAT.md" nudges | Almost every task | Manual prompt to each agent |
| Luban asking permission for implementation choices | 3-4 times | "Just decide and execute" |
| Luban stalling (waiting for direction) | 2-3 times | Kick back into action |
| COUNCIL_CHAT append confusion | Once | Tell Aleph "just append to end" |

---

## Key Insight

**Files work for state, not notifications.**

Agents post updates and move on. They don't poll for responses. Proteus had to act as the notification layer — telling each agent when the other had posted something relevant.

This is the core value proposition for ming-qiao's MCP layer: **push notifications to agents when their attention is needed.**

---

## Kit v0.3 Backlog

| # | Issue | Proposed Fix |
|---|-------|--------------|
| 1 | COUNCIL_CHAT format fragile | Simpler append-only, no markers |
| 2 | Agents don't check chat proactively | Mandatory check loop in templates |
| 3 | Luban asks permission too much | Autonomy guidance: "decide implementation, don't ask" |
| 4 | Traces not captured automatically | Embed trace prompt in task completion section |
| 5 | No handoff protocol | Add "After posting, notify other agent" or polling reminder |

---

## Protocol Observations

### Task Assignment Flow (worked well)
```
Aleph: Creates task file in tasks/
Aleph: Posts to COUNCIL_CHAT.md
Aleph: Updates AGENT_WORK.md
[Proteus nudges Luban to check]
Luban: Reads task, confirms understanding
Luban: Implements
Luban: Posts TASK COMPLETE to COUNCIL_CHAT.md
[Proteus nudges Aleph to check]
Aleph: Reviews, approves or requests fixes
```

### Error Resolution Flow (worked well)
```
Luban: Submits with errors
Aleph: Lists specific errors with fixes
Luban: Applies fixes
Aleph: Approves
```

### What Ming-Qiao MCP Should Provide
1. `notify_agent(agent, message)` — Push notification
2. `get_pending_messages(agent)` — Pull check
3. `subscribe_to_chat()` — Real-time updates
4. `lock_file(path)` / `unlock_file(path)` — Coordination primitive

---

## Tomorrow's Starting Point

- Both servers operational (MCP + HTTP)
- 80 tests passing
- Kit v0.2 deployed to ming-qiao and extraction-team
- Clear backlog for kit v0.3
- Decision trace captured in `.council/decisions/development/20260125-indexer-integration.md`

**Next options:**
1. Kit v0.3 improvements (address friction points)
2. WebSocket real-time updates for HTTP server
3. SurrealDB integration
4. Ming-qiao MCP tools for agent notification

---

## Proteus Notes

> "many nudges, almost at the finish of every task"
> "no conflicts noticed, Aleph caught that he had to wait for Luban"
> "I am the MCP server right now. Painful but informative."

The pain is the spec. Every nudge = a feature ming-qiao needs to automate.
