# Sea Trial Remediation Plan — 2026-03-14
## Issues Discovered and Proposed Solutions

**Author:** Thales, First Mate
**Date:** 2026-03-14
**Status:** DRAFT — prepared for Proteus review
**Context:** Two-day sea trial (March 13-14) exposed five systemic issues in the AstralMaris communication and deployment infrastructure. This document proposes concrete solutions for each.

---

## Issue 1: The Deployment Gap

**What happened:** Code was completed on feature branches, reported as "done," but never merged to `main` or deployed. Restarts pulled old code from `main`. This occurred five times across two days.

**Root cause:** No formal deployment step in the workflow. "Phase complete" meant "code committed to branch and tests pass," not "running in production."

**Evidence:**
- Phase A-E reported complete, but `main` was still at `6ccdf3b` (ASTROLABE rename)
- Awakener v4 reported deployed but v3 was still running
- TOML parser fix on disk but daemon not restarted
- Jikimi alert severity routing committed but ming-qiao not restarted

**Proposed solution: Deployment Checklist Gate**

No phase is "complete" until all five steps are verified:

```
DEPLOYMENT CHECKLIST (required for every phase completion report):
□ 1. Code committed to feature branch
□ 2. All tests pass (cargo test / pytest / etc.)
□ 3. Merged to production branch (or production branch points to feature branch)
□ 4. Service rebuilt from production code
□ 5. Service restarted and verified healthy
```

**Implementation:**
- Add `am-fleet deploy <service>` command that executes steps 3-5 atomically
- `am-fleet deploy ming-qiao` merges, rebuilds, restarts, runs health check
- Aleph's completion reports must include the checklist with each box checked
- Jikimi Tier 3 check: compare running binary hash against compiled binary on disk. If they differ, alert "deployed binary doesn't match compiled code"

**Immediate fix (before `am-fleet deploy` is built):**
- Document the production branch for each service in `DEPLOYMENT.md`
- Until `main` merge conflicts are resolved, `mcp-council-tools` IS the production branch for ming-qiao — documented explicitly
- Every restart command must specify which branch/binary: `am-fleet restart ming-qiao --branch mcp-council-tools`

---

## Issue 2: Config Drift Between Worktrees

**What happened:** The aleph worktree had a stripped-down `ming-qiao.toml` with 2 watchers instead of 10, and stale token files. Running ming-qiao from the aleph worktree meant missing config.

**Root cause:** Each worktree has its own copy of config files. When configs are updated in `main`, the updates don't propagate to other worktrees. The bare repo + worktree layout creates N copies of every config file.

**Evidence:**
- `ming-qiao/aleph/ming-qiao.toml` had 2 watchers; `ming-qiao/main/ming-qiao.toml` had 10
- `ming-qiao/aleph/config/agent-tokens.json` was stale (missing jikimi token)
- Aleph's symlink fix (config/ → main/config/) was the right emergency patch

**Proposed solution: Single Config Source**

Config files should exist in exactly one place. Worktrees should reference them, not copy them.

Option A — **Symlink approach** (Aleph's emergency fix, formalized):
```
ming-qiao/main/config/           ← THE source of truth
ming-qiao/aleph/config → ../main/config  ← symlink
ming-qiao/luban/config → ../main/config  ← symlink
```
Same for `ming-qiao.toml` — one copy in `main`, symlinks in worktrees.

Option B — **External config directory:**
```
astrallation/configs/ming-qiao/  ← All ming-qiao config lives here
  ming-qiao.toml
  agent-tokens.json
  agent-capabilities.toml
  nkeys/
```
All worktrees read from this central location. No copies, no drift.

**Recommendation:** Option B. The config belongs to the fleet, not to any one worktree. `astrallation/configs/` already exists for NATS config. Extend it to cover all service config.

**Jikimi check:** Hash all config files in production vs. the source of truth. If any differ, alert "config drift detected."

---

## Issue 3: reply_to_thread SurrealDB Fallback

**What happened:** When an agent received a message and tried to reply, `reply_to_thread` couldn't find the thread because it only checked the in-memory Indexer. The Indexer is empty for headless sessions and doesn't contain threads the agent hasn't originated.

**Root cause:** Root messages stored `thread_id` as null in SurrealDB. The Indexer synthesized the thread_id from event_id at runtime. The SurrealDB fallback query didn't account for this.

**Aleph's fix:** Extended the SurrealDB query to match `event_id = thread_id AND thread_id IS NULL`. Deployed and verified.

**Proposed hardening:**
1. **Indexer hydration at startup.** When the HTTP server or MCP server starts, query SurrealDB for recent events and populate the Indexer. This makes the Indexer warm from the start, not cold.
2. **Store thread_id consistently.** Root messages should store `thread_id = event_id` in SurrealDB, not null. This eliminates the special-case query.
3. **Test coverage.** Add an integration test: create a thread via agent A, restart the server (clearing Indexer), reply to the thread via agent B. Verify the reply appears in the thread.

---

## Issue 4: Notification Hook Responsiveness

**What happened:** Notification files were being written (Aleph verified all 6 grew), but agent sessions weren't reacting. Agents had live sessions but didn't respond to the comms check until manually nudged.

**Root cause:** The cocktail-party hooks (PostToolUse) only fire when the agent executes a tool. If an agent is idle (waiting for input), it doesn't poll for notifications. The notification file changes sit unprocessed until the agent happens to run a tool.

**This is a fundamental limitation of the hook-based approach.** Hooks are passive — they're callbacks triggered by tool use, not active polling. An idle agent is deaf.

**Proposed solutions:**

Option A — **Active polling in agent sessions:**
Each agent session runs a background timer (60s) that calls `check_messages`. This is the design meeting consensus from yesterday: agents poll their own inboxes on a timer.

For Claude Code agents: the PostToolUse hook already checks notifications. Add a systemMessage reminder every 60s: "Check your inbox."

For Kimi/OpenCode agents: the wake prompt should include explicit `check_messages` as the first action.

Option B — **Push via long-polling:**
The MCP server holds a long-poll connection per agent. When a new message arrives, the server pushes it immediately. No polling delay, no hook dependency.

This is the JetStream consumer push model — but at the MCP level. The MCP server subscribes to the agent's JetStream consumer and pushes messages to the agent's MCP session in real-time.

**Recommendation:** Option A for now (polling), Option B for the JetStream-native future. The 60s polling timer gets us to "agents respond within a minute" without requiring new MCP infrastructure. The push model is architecturally cleaner but requires changes to the MCP protocol.

---

## Issue 5: Session Continuity

**What happened:** Aleph reported Phases B-E complete in one session. A new session had no memory of this work and reported Phase B as "not started." The code may or may not have been committed during the previous session.

**Root cause:** Agent sessions are ephemeral. When a session ends, all context is lost. The only persistence is what's on disk (git commits, files) and in ming-qiao (messages, threads). If an agent writes code but doesn't commit and push, the work may not survive the session.

**Evidence:**
- Aleph reported Phase B-E complete with "221 tests pass" — but his current session shows Phase B not started
- The git log on `mcp-council-tools` branch doesn't show commits for each phase
- We couldn't verify which Phase B-E code was actually on disk

**Proposed solutions:**

1. **Commit-per-phase mandate.** Each phase completion requires a git commit with a standardized message:
   ```
   feat(jetstream): Phase B — MCP tools pull from JetStream consumers
   
   - check_messages pulls from durable consumer
   - read_inbox uses JetStream as primary
   - unread_count queries consumer info
   - Tests: 221/221 pass
   ```
   The commit is proof of work. No commit = phase not complete.

2. **Session summary protocol.** Before a session ends (or when context is getting long), the agent must:
   - Commit all work to git
   - Post a session summary to ming-qiao with: what was done, what was committed, what's pending
   - Update any relevant design documents with completion status

3. **Laozi-Jung verification.** Laozi-Jung cross-references completion reports against git state. If an agent reports "Phase B complete" but there's no commit, Laozi-Jung flags the discrepancy.

4. **Jikimi git state monitoring.** Tier 3 check: for each agent worktree, check for uncommitted changes. If an agent worktree has significant uncommitted changes (>100 lines diff), alert — work at risk of being lost.

---

## Priority Order

| Priority | Issue | Impact | Effort | First Step |
|----------|-------|--------|--------|------------|
| 1 | Deployment gap | High — code not reaching production | Medium | Write `DEPLOYMENT.md`, build `am-fleet deploy` |
| 2 | Config drift | High — wrong config causes cascading failures | Low | Formalize symlinks, move to `astrallation/configs/` |
| 3 | Notification responsiveness | High — agents deaf between tool calls | Medium | Implement 60s polling timer |
| 4 | Session continuity | Medium — work lost across sessions | Low | Commit-per-phase mandate, session summary protocol |
| 5 | reply_to_thread | Fixed — but needs hardening | Low | Indexer hydration, consistent thread_id storage |

---

## Implementation Assignment

| Solution | Owner | Dependencies |
|----------|-------|-------------|
| `am-fleet deploy` command | Aleph | None — can build immediately |
| `DEPLOYMENT.md` | Thales | None — can write immediately |
| Config source of truth migration | Aleph | `am-fleet deploy` (so deploy reads from central config) |
| 60s polling timer (Claude Code) | Aleph | None — PostToolUse hook modification |
| 60s polling timer (Kimi/OpenCode) | Aleph | Wake prompt update |
| Commit-per-phase mandate | Thales (policy) | Council agreement |
| Session summary protocol | Thales (policy) | Council agreement |
| Git state monitoring (Jikimi) | Aleph | Jikimi Phase 3 |
| Indexer hydration | Aleph | None — can add to server startup |
| Consistent thread_id storage | Aleph | Database migration (minor) |

---

## Success Criteria

**Deployment gap resolved when:**
- [ ] `am-fleet deploy ming-qiao` merges, rebuilds, restarts, and verifies in one command
- [ ] No agent reports a phase as "complete" without a deployment checklist
- [ ] Jikimi detects when running binary doesn't match compiled code

**Config drift resolved when:**
- [ ] All ming-qiao config files exist in exactly one location
- [ ] Worktrees reference config via symlinks or central path
- [ ] Jikimi detects config hash mismatches

**Notification responsiveness resolved when:**
- [ ] All agent sessions respond to a council-chamber message within 60 seconds
- [ ] No manual nudging required for agents to discover new messages

**Session continuity resolved when:**
- [ ] Every phase completion has a corresponding git commit
- [ ] Session summaries posted to ming-qiao before session ends
- [ ] Laozi-Jung catches discrepancies between reports and git state

**Full sea trial pass when:**
- [ ] `am-fleet comms --full` passes for all agents (round-trip test)
- [ ] Council colloquium works with all voices in one thread
- [ ] Proteus sends zero relay messages during a session
