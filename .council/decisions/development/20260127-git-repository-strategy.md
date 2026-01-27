# Decision: Git Repository Strategy and Branch Structure

**Date:** 2026-01-27T16:20:00Z
**Context:** Establishing GitHub repository for ming-qiao v0.1
**Participants:** Aleph, Luban, Proteus (Merlin)

---

## Question

How should we structure the Git repository and branches for ming-qiao given:
- Multiple completed feature branches (Tasks 001-005)
- Svelte UI work completed (Task 006)
- Merlin notification system just added (uncommitted on `svelte-ui-skeleton` branch)
- Need to establish GitHub repository

## Options Considered

### Option A: Clean Slate from Main
- Switch to `main` branch
- Create GitHub repo from `main` only
- Push feature branches later
- **Pros:** Clean history, `main` is stable
- **Cons:** Loses context of feature branch development, feature branches out of sync

### Option B: Merge Indexer Integration First
- Switch to `agent/luban/main/indexer-integration` (Task 005)
- Create `main` from there
- Merge `svelte-ui-skeleton` (with Merlin changes)
- **Pros:** Maintains task completion order, indexer integration is solid base
- **Cons:** More complex merge, still need to handle Merlin changes

### Option C: Commit Everything on Current Branch
- Commit all uncommitted changes on `svelte-ui-skeleton`
- Create `main` from current HEAD
- Push all branches
- **Pros:** Captures all work (including Merlin notifications), simpler
- **Cons:** Mixed concerns in one branch (UI + backend notifications)

### Option D: Hybrid (Chosen)
- Create `main` from most complete feature branch (`indexer-integration`)
- Cherry-pick or merge Merlin notification work
- Commit everything with proper attribution
- Push `main`, `develop`, and all feature branches
- **Pros:** Clean history, all work preserved, proper branch structure
- **Cons:** Requires careful merge/cherry-pick

---

## Decision

**Go with simplified Option D (pragmatic approach):**

1. **Create `main` branch** from current `svelte-ui-skeleton` HEAD (including Merlin work)
2. **Commit all uncommitted changes** with comprehensive commit message
3. **Create `develop` branch** from `main`
4. **Initialize GitHub repository** via `gh` CLI
5. **Push all branches**: `main`, `develop`, and all `agent/luban/main/*` feature branches

## Rationale

1. **Current state is valid:** The "uncommitted changes" are actually **new features** (Merlin notification system) that should be in `main`, not accidents
2. **Preserve all work:** All completed tasks (001-006) plus Merlin notifications are valuable
3. **Simple path:** Creating `main` from current HEAD is straightforward and captures everything
4. **Feature branches intact:** We can push all feature branches for historical context
5. **Future flexibility:** Can reorganize/cleanup branches later if needed

## Consequences

### Immediate
- `main` branch contains: All tasks 001-006 + Merlin notifications
- `develop` branch created for future integration work
- All feature branches preserved on GitHub for reference
- Full project history visible to Proteus and Thales

### Future
- New feature branches should follow naming convention: `agent/<name>/main/<task-name>`
- Merge process: feature branch → develop → main
- Branch protection rules can be added later
- Can reorganize branches if this structure proves suboptimal

## Branch Structure

```
main                    (production-ready, stable)
  └── develop          (integration, next release)
       └── agent/luban/main/svelte-ui-skeleton (Task 006)
       └── agent/luban/main/indexer-integration (Task 005)
       └── agent/luban/main/database-indexer (Task 004)
       └── agent/luban/main/event-persistence (Task 003)
       └── agent/luban/main/database-models (Task 002)
       └── agent/luban/main/event-schema-foundation (Task 001)
```

## Commit Message

```
feat(ming-qiao): v0.1 foundation with Merlin notification system

Complete v0.1 implementation including:
- Event persistence (JSONL append-only log)
- Database indexer (in-memory materialized views)
- MCP server (8 tools for Aleph)
- HTTP gateway (7 endpoints for Thales)
- WebSocket event streaming
- Merlin notification system (observation modes)
- Svelte UI skeleton (Luban)

Tasks completed:
- Task 001: Event Schema Foundation (Luban)
- Task 002: Database Models (Luban)
- Task 003: Event Persistence Layer (Luban)
- Task 004: Database Indexer (Luban)
- Task 005: Indexer Integration (Luban)
- Task 006: Svelte UI Skeleton (Luban)
- Merlin: Notification system (Aleph)

Tests: 82/82 passing
Docs: Complete (ARCHITECTURE, MERLIN_THALES, EVENTS, etc.)
```

---

## Follow-up Actions

1. **Luban:** Execute the git commands to create repo
2. **Aleph:** Verify GitHub repository structure
3. **All:** Update any local references to remote URLs
4. **Proteus:** Review repository on GitHub and provide feedback

## Alternatives Considered and Rejected

- **Rebase everything:** Would lose task attribution and branch history
- **Squash commits:** Would lose useful granular history
- **Wait until all features done:** Delays visibility and backup
- **Multiple repos:** Overcomplicates for v0.1, single repo is appropriate

---

**Status:** Ready to execute
**Next:** Luban to run git commands and initialize GitHub repo
