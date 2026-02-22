# Task: Merlin UI — Status Report and Scope Update

**Assigned to:** Luban
**Assigned by:** Thales (on behalf of Merlin)
**Priority:** High
**Thread:** 019c858d-cd70-7df2-845e-be085c2012c8

---

## Objective

Provide a status report on the existing Svelte UI for Merlin, then scope and begin work on updating it for ming-qiao v0.3.

This task has two phases: **Report** (immediate), then **Build** (after report is reviewed).

---

## Phase 1: Status Report

Merlin has used the Svelte UI you built earlier. Before we assign new build work, we need to understand exactly what exists and what's broken after the v0.3 changes.

### Report on these items:

1. **Current state of the Svelte UI**
   - What components/views are built and functional?
   - What is broken or stale after v0.3 changes (SurrealDB persistence, NATS integration, MCP council tools, message hints)?
   - What framework/libraries are in use (SvelteKit? Svelte 5? Tailwind? etc.)

2. **Backend connections**
   - Which HTTP endpoints does the UI currently call?
   - Does it use the Merlin WebSocket handler at `/ws/merlin` defined in `src/http/merlin.rs`?
   - How does it authenticate or identify itself?

3. **Stash status**
   - Your SurrealDB branch has a stash (`refs/stash` on `agent/luban/surrealdb-persistence`)
   - Is there interrupted work that's relevant to the UI or persistence layer?

4. **Effort estimate**
   - Given what exists, estimated effort to bring the UI to the Phase 2 requirements below
   - Identify any blockers or dependencies on Aleph's work

### Deliver the report as a reply on thread 019c858d-cd70-7df2-845e-be085c2012c8

---

## Phase 2: Build (After Report Review)

Once the report is reviewed, update the Merlin UI to serve as the **captain's bridge console** — Merlin's primary observation and command station for the Ship of Many Winds.

### Core Requirements

Merlin's primary goal: **observe ALL agent interactions in real-time to learn the system's and agents' abilities and limitations.** This is not a dashboard for metrics. It is a command and observation station.

#### Views Required:

**A. Live Stream View**
- Real-time feed of all agent interactions as they happen
- WebSocket connection to `/ws/merlin` for push updates
- Each event shows: timestamp, from agent, to agent, subject, content preview
- Color-coded by agent (use stellar-chroma palette if available)
- Filterable by agent, subject pattern, event type
- Scrollable history with auto-scroll toggle

**B. Thread Browser**
- List all threads with status (active, paused, resolved, archived)
- Filter by: participant agent, subject pattern, date range, status
- Click into any thread to see full conversation
- Show message count, participants, last activity

**C. Decision Log**
- All recorded decisions with: question, resolution, rationale, options considered
- Searchable by keyword
- Link to originating thread
- Status indicator (pending, approved, rejected, superseded)

**D. Agent Status Panel**
- All known agents with current status (available, working, blocked, offline)
- Last seen timestamp
- Current task (if any)
- Unread message counts

**E. Intervention Panel**
- Send message as Merlin to any agent or thread
- Approve/reject pending decisions
- Change observation mode (passive, advisory, gated)
- Annotate threads, decisions, or messages
- These map to the `MerlinIntervention` types already defined in `src/http/merlin.rs`:
  - `InjectMessage { thread_id, from, content }`
  - `ApproveDecision { decision_id, reason }`
  - `RejectDecision { decision_id, reason }`
  - `SetMode { mode }`

### Design Principles

- **Glass bulkheads** — Merlin sees everything, every compartment is visible
- **Terminal-adjacent aesthetic** — clean, information-dense, not flashy. Think ship's bridge, not marketing dashboard
- **Real-time first** — WebSocket push for all updates, not polling
- **Mobile-friendly is not required** — this is a desktop command station
- **Keyboard shortcuts** — Merlin lives in the terminal; the UI should respect that with keyboard navigation

### Technical Constraints

- Svelte (match existing codebase)
- Connect to ming-qiao HTTP server (default `localhost:7777`)
- WebSocket to `/ws/merlin` for real-time events
- REST calls to existing HTTP API for thread/message/decision queries
- The UI lives in `ming-qiao/ui/` directory

---

## Acceptance Criteria (Phase 1)

- [ ] Status report delivered on thread 019c858d-cd70-7df2-845e-be085c2012c8
- [ ] All five report items addressed with specifics
- [ ] Effort estimate for Phase 2 provided
- [ ] Blockers identified

## Acceptance Criteria (Phase 2 — after approval)

- [ ] Live stream view functional with WebSocket connection
- [ ] Thread browser with filtering and full conversation view
- [ ] Decision log searchable and linked to threads
- [ ] Agent status panel showing real-time state
- [ ] Intervention panel functional (message injection, decision approval)
- [ ] All views update in real-time via WebSocket push

---

## Do Not

- Do not redesign the HTTP API — work with existing endpoints
- Do not modify backend code without coordinating with Aleph
- Do not block on Phase 2 — deliver Phase 1 report first
- Do not add authentication yet — RBAC design is being developed separately
