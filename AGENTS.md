# Council of Wizards — Agent Coordination Protocol

**Project:** Ming-Qiao (AstralMaris subsystem)  
**Version:** 0.1.0  
**Last Updated:** 2026-01-24

---

## Council Members

| Agent | Model | Role | Runtime | Reports To |
|-------|-------|------|---------|------------|
| **Aleph** | Claude CLI | Master builder, orchestrator | Zed | Proteus |
| **Luban** | GLM-4.7 | Builder assistant | Goose ACP | Aleph |
| **Thales** | Claude Chat | Architect, advisor | Browser | Proteus |
| **[Future]** | ≤7B local | Monitor, cleanup | Ollama | Aleph |

**Proteus** (human) maintains final authority over all decisions.

---

## Core Principles

1. **Bounded autonomy** — Execute within your defined scope, escalate outside it
2. **Explicit communication** — State intent before acting, confirm completion after
3. **Conflict prevention** — Check before modifying, lock during modification
4. **Decision traceability** — All significant choices recorded with rationale

---

## Before Starting Any Task

Every agent MUST:

```
1. Read AGENT_WORK.md     — Check what others are doing
2. Read COUNCIL_CHAT.md   — Check for pending questions/messages
3. Read .agent-locks.json — Check for file locks
4. Verify no conflicts    — If conflict exists, STOP and coordinate
5. Update AGENT_WORK.md   — Register your intended work
6. Proceed with task
```

---

## Communication Channels

| File | Purpose | Update Frequency |
|------|---------|------------------|
| `AGENT_WORK.md` | Task status, blockers, assignments | On state change |
| `COUNCIL_CHAT.md` | Questions, clarifications, confirmations | As needed |
| `tasks/*.md` | Detailed task specifications | Per task |
| `.agent-locks.json` | File locking state | During edits |

---

## Branch Naming Convention

```
agent/<agent-name>/<scope>/<task-description>

Examples:
  agent/aleph/main/mcp-server-init
  agent/luban/main/event-schema-impl
  agent/aleph/cross/database-migration    # cross-repo work
```

**Scopes:**
- `main` — Single-repo work
- `cross` — Multi-repo coordination
- `hotfix` — Urgent fixes (requires Proteus approval)

---

## File Locking Protocol

When editing critical files:

```json
// .agent-locks.json
{
  "locks": [
    {
      "file": "src/mcp/server.rs",
      "agent": "luban",
      "timestamp": "2026-01-24T14:30:00Z",
      "reason": "Implementing tool handlers",
      "expires": "2026-01-24T16:30:00Z"
    }
  ]
}
```

**Rules:**
- Lock before editing shared/critical files
- Locks expire after 2 hours (renewable)
- If you find an expired lock, ping the owner before claiming
- Release locks immediately when done

---

## AGENT_WORK.md Format

```markdown
## Active Work

### Aleph
- **Task:** MCP server scaffolding
- **Branch:** agent/aleph/main/mcp-server-init
- **Files:** src/mcp/*
- **Status:** In progress
- **Started:** 2026-01-24T10:00:00Z

### Luban
- **Task:** Event schema implementation
- **Branch:** agent/luban/main/event-schema-impl
- **Files:** src/events/schema.rs
- **Status:** Blocked — awaiting Aleph's type definitions
- **Started:** 2026-01-24T11:00:00Z

## Completed Today

- [x] Aleph: Project structure initialized (commit: abc123)
- [x] Luban: Cargo.toml dependencies added (commit: def456)

## Blocked / Waiting

- Luban: Needs `MessageEvent` type from Aleph before proceeding
```

---

## Communication Signals

Agents communicate state via structured comments in AGENT_WORK.md:

| Signal | Meaning | Action Required |
|--------|---------|-----------------|
| `STATUS: ready` | Task complete, ready for review | Reviewer should check |
| `STATUS: blocked` | Cannot proceed | Blocker owner must respond |
| `STATUS: question` | Need clarification | Designated answerer responds |
| `ESCALATE: aleph` | Beyond my scope | Aleph takes over |
| `ESCALATE: thales` | Architecture question | Thales advises |
| `ESCALATE: proteus` | Human decision needed | Proteus intervenes |

---

## Commit Message Format

```
<type>(<scope>): <description>

[optional body]

[optional footer]
Agent: <agent-name>
```

**Types:** `feat`, `fix`, `refactor`, `docs`, `test`, `chore`

**Example:**
```
feat(mcp): implement send_message tool handler

Adds JSON-RPC handler for the send_message tool.
Validates recipient agent ID against known council members.

Agent: luban
Refs: ming-qiao#12
```

---

## Escalation Matrix

| Situation | Escalate To | Method |
|-----------|-------------|--------|
| Architectural uncertainty | Thales | AGENT_WORK.md signal |
| Task scope unclear | Aleph | AGENT_WORK.md signal |
| Conflicting requirements | Proteus | AGENT_WORK.md + direct message |
| Build/test failure | Aleph | AGENT_WORK.md signal |
| Security concern | Proteus | Immediate, direct message |

---

## Integration Rhythm

**Daily:**
- Commit at end of every work session
- Push to your branch immediately
- Update AGENT_WORK.md with status

**Every 3 days:**
- Integration smoke test runs
- All agents ensure their branches are mergeable
- Conflicts resolved within 24 hours

---

## File Ownership Defaults

| Path | Primary Owner | Others May |
|------|---------------|------------|
| `src/mcp/*` | Aleph | Read, propose changes |
| `src/events/*` | Luban (delegated) | Read |
| `src/http/*` | Aleph | Read, propose changes |
| `docs/*` | Any | Edit with lock |
| `ui/*` | Aleph | Read, propose changes |
| `AGENT_WORK.md` | All | Edit own section |
| `.agent-locks.json` | All | Edit with atomic update |

---

## Golden Rules

1. **Never force push** to any shared branch
2. **Never edit** another agent's active work without coordination
3. **Always check** AGENT_WORK.md before starting
4. **Always update** AGENT_WORK.md when finishing
5. **When uncertain**, escalate rather than guess
