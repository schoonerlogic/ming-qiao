# Decision: Kit Deployment Learnings

**Date:** 2026-01-25  
**Status:** accepted  
**Deciders:** Aleph, Proteus

## Context

First deployment of the AstralMaris Agent Kit to an existing project (ming-qiao). This document captures learnings and feedback for Thales to improve the kit.

## Deployment Summary

**What worked well:**
- Existing ming-qiao coordination files (AGENTS.md, AGENT_WORK.md, agent instructions) were superior to kit templates — they were already customized and battle-tested
- Kit's decision trace infrastructure (.council/, docs/DECISION_TRACES.md) filled a real gap
- File-based coordination protocol is solid

**What was added:**
- `.agent-locks.json` — was missing
- `.council/` directory with config.yaml and schemas
- `docs/DECISION_TRACES.md` and `docs/TRACE_CAPTURE.md`
- `docs/decisions/` directory with ADR template
- `tasks/` directory (moved existing task file into it)

**What was NOT overwritten:**
- AGENTS.md — existing was more detailed and project-specific
- AGENT_WORK.md — had real work history
- COUNCIL_CHAT.md — had real conversation history
- .goosehints — already properly configured
- Agent instruction files — complete and customized

## Feedback for Kit Improvements

### 1. setup.sh Should Be Smarter

**Problem:** Running `setup.sh` blindly would have overwritten the excellent existing files with generic templates.

**Recommendation:** Add `--merge` or `--additive` mode that:
- Only creates files that don't exist
- Warns about existing files instead of overwriting
- Offers to show diffs for review

Example:
```bash
./setup.sh ming-qiao ~/ming-qiao --merge
# Output:
# Skipping AGENTS.md (exists)
# Skipping AGENT_WORK.md (exists)  
# Creating .council/config.yaml
# Creating docs/DECISION_TRACES.md
# ...
```

### 2. Kit CLAUDE.md Identity Confusion

**Problem:** The kit's `/CLAUDE.md` has two conflicting contexts:
1. "Aleph as master builder on ming-qiao"
2. "Working in the kit repo for deployment"

When deploying FROM the kit TO a project, which Aleph am I? The instruction overlap created confusion.

**Recommendation:** 
- Kit's CLAUDE.md should focus purely on "kit maintainer" tasks
- Project-specific Aleph instructions should be templates in `agents/_template/`
- Make the boundary clearer

### 3. Placeholder Replacement Incomplete

**Problem:** `[PROJECT_NAME]` was replaced but `[REPO_URL]` and `[TIMESTAMP]` were not handled in all files.

**Recommendation:** 
- Add `[REPO_URL]` to setup.sh parameter list, or
- Auto-detect from `git remote -v`, or
- Leave as `local:<project-name>` with comment explaining how to update

### 4. Task File Location Ambiguity

**Problem:** Ming-qiao had `001-event-schema-foundation.md` in root instead of `tasks/`. The AGENT_WORK.md referenced `tasks/001-...` but the file was elsewhere.

**Recommendation:** 
- Kit should document that task files go in `tasks/`
- Task template should be more prominent
- Consider adding a "Task Management" section to AGENTS.md

### 5. Decision Trace vs ADR Relationship Unclear

**Problem:** Now we have:
- `docs/decisions/` — Human-readable ADRs
- `.council/decisions/` — Machine-readable traces

The relationship and when to use which isn't immediately obvious.

**Recommendation:** Add a "Decision Recording" section to AGENTS.md explaining:
- Quick implementation decisions → trace only
- Architectural decisions → ADR + trace
- When to use each

### 6. agents/ Directory Structure

**Observation:** Kit has `agents/_template/AGENT.md` but ming-qiao evolved to have:
- `agents/aleph/CLAUDE.md` (Claude CLI)
- `agents/luban/AGENT.md` (Goose)
- `agents/thales/CONTEXT.md` (Claude Chat)

Different file names for different runtimes is sensible.

**Recommendation:** Document this naming convention in `agents/README.md`:
- Claude CLI → `CLAUDE.md`
- Goose → `AGENT.md`  
- Claude Chat → `CONTEXT.md`
- Generic → `INSTRUCTIONS.md`

### 7. Root CLAUDE.md Duplication

**Observation:** Root `CLAUDE.md` is identical to `agents/aleph/CLAUDE.md`. This creates maintenance burden.

**Options:**
1. Keep only root CLAUDE.md (Claude CLI reads it)
2. Keep only agents/aleph/CLAUDE.md and symlink
3. Accept duplication (current state)

**Recommendation:** Use symlink or document that root CLAUDE.md is authoritative.

## Consequences

These learnings will improve future kit deployments. Thales should review and incorporate into kit v0.2.

## Related

- `.council/config.yaml` — Project configuration
- `docs/DECISION_TRACES.md` — Trace documentation
- `agents/*/` — Agent instruction files
