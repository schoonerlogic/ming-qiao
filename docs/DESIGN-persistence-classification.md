# AstralMaris Persistence Classification Design

**Author:** Thales (Architect)
**Date:** 2026-02-22
**Status:** Draft for Council Review
**Scope:** All AstralMaris data produced by agent coordination, development activity, and system observation

---

## The Problem

AstralMaris produces several categories of data with fundamentally different lifecycles, access patterns, and long-term value. Today, everything flows into SurrealDB (operational state) or git (code and documents) without explicit classification. As the system scales — more agents, more interactions, more observation streams — we need clear boundaries around what data goes where, how long it lives, who can access it, and what it eventually becomes.

This is the glass bulkheads design. Every compartment is visible, but the water stays where it belongs.

---

## Classification Tiers

### Tier 1: Operational Stream (Hot)

**What it is:** Real-time agent coordination data. Messages, thread state, presence heartbeats, task assignments, status updates. This is the nervous system — signals in flight.

**Current location:** SurrealDB (in-memory or `ws://localhost:8000`), NATS subjects

**Characteristics:**
- High volume, high velocity
- Value decays rapidly (most messages are operationally useful for hours to days)
- Must be queryable in real-time (agent inboxes, thread views, unread counts)
- Tolerates loss of old data (the system rebuilds state from recent events)

**Retention policy:** 30 days in SurrealDB. Events older than 30 days are eligible for archival or deletion. Presence heartbeats: 24 hours (ephemeral by design, already noted in persistence.rs).

**Access:** All council agents (read), participants (write to own threads), Merlin (full access + intervention)

**What lives here:**
- `event` table (EventEnvelopes — the append-only event log)
- `presence` table (agent heartbeats)
- `task_assignment`, `task_status_update` tables
- `session_note` table
- Materialized views: Thread, Message, Agent state (rebuilt from events by Indexer)

**What does NOT live here:**
- Decisions (promoted to Tier 2 on creation)
- Design documents, proposals, ADRs
- Witness observations
- Training data partitions

---

### Tier 2: Decision Record (Durable)

**What it is:** Formal decisions with rationale, options considered, and resolution. These are the institutional memory — the "why" behind the system's evolution. Also includes design artifacts, council deliberations, and architectural proposals.

**Current locations:** SurrealDB `event` table (DecisionRecorded events), git (council-deliberations/, .council/decisions/, ADRs)

**Characteristics:**
- Low volume, permanent value
- Must survive infrastructure changes (SurrealDB restarts, machine migrations)
- Must be searchable by topic, date, agent, and eventually by semantic similarity
- Forms the causal chain — decisions reference upstream decisions and threads
- This is the raw material for Council's future encoding layer

**Retention policy:** Permanent. Decisions are never deleted. They may be superseded (DecisionStatus::Superseded) but the original record persists.

**Access:** All council agents (read), decision participants (record), Merlin (approve/reject/annotate), Observers (read-only)

**Promotion trigger:** Every `DecisionRecorded` event is automatically promoted from Tier 1 to Tier 2. The event remains in SurrealDB for operational access, AND is written to durable storage.

**Durable storage format:** Each decision as a YAML file in git, following the existing `.council/decisions/` convention:

```
.council/decisions/
├── 2026-02-21-observation-architecture.yaml
├── 2026-02-21-bridge-confirmed.yaml
└── ...
```

The YAML includes: decision_id, thread_id (link to originating conversation), question, resolution, rationale, options_considered, recorded_by, created_at, status, and upstream_decisions (causal chain).

**What lives here:**
- All `record_decision` outputs (dual-written: SurrealDB + git YAML)
- Council deliberation documents (already in git)
- Architecture Decision Records (already in git: `platform/docs/architecture/decisions/`)
- Design proposals (already in git: `council-deliberations/`)
- Merlin annotations on decisions

**Relationship to Tier 1:** Decisions are born in Tier 1 (as events in the stream) and promoted to Tier 2 on creation. The Tier 1 event is the receipt; the Tier 2 YAML is the record.

---

### Tier 3: Witness Archive (Curated)

**What it is:** Laozi-Jung's observations, pattern synthesis, and institutional memory. The elder's perspective on what the system is doing, what it's not doing, and what it means.

**Current location:** echoessence repository (git)

**Characteristics:**
- Medium volume (daily observations + real-time pattern notes)
- Growing value over time (patterns compound)
- Human-curated signal, not raw data
- Cross-references Tier 1 events and Tier 2 decisions
- Forms the "wisdom layer" — not what happened, but what it means

**Retention policy:** Permanent in git. Observations are never deleted. They may be annotated or revised.

**Access:** Laozi-Jung (write), all agents (read), Merlin (read + annotate)

**What lives here:**
- Daily witness notes (`observations/daily/YYYY-MM-DD.md`)
- Pattern observations (`observations/patterns/`)
- Agent profiles (`agent-profiles/{agent}.md`)
- Project essence views (`project-status/{project}.md`)
- Onboarding briefs (`onboarding/generated/`)
- The real-time event stream JSONL (`observations/stream/stream.jsonl`) — this is Tier 1 data flowing through Laozi-Jung's observation post, retained as raw input for pattern synthesis

**Relationship to Tier 1:** The watcher system feeds Tier 1 events to Laozi-Jung as JSONL. This raw stream is the input; the witness observations are the output. The stream JSONL itself can be rotated (30 days) since its value is extracted into observations.

**Relationship to Tier 2:** Witness observations frequently reference decisions ("The council decided X in thread Y — here is what happened next"). These cross-references form the narrative layer on top of the decision record.

---

### Tier 4: Artifact Registry (Versioned)

**What it is:** Shared artifacts between agents — files, documents, code snippets, generated outputs. Things with checksums and paths.

**Current location:** SurrealDB (ArtifactEvent metadata), local filesystem (actual files), git (committed artifacts)

**Characteristics:**
- Variable volume and size
- Value depends on context (a shared config file vs. a generated report)
- Requires integrity verification (checksums)
- May reference Tier 1 threads (context for why artifact was shared)
- May be inputs to or outputs of Tier 2 decisions

**Retention policy:** Metadata permanent in SurrealDB. Actual files follow the lifecycle of the project they belong to. Artifacts referenced by Tier 2 decisions must be preserved alongside the decision.

**Access:** Sharing agent (write), target agent (read), Merlin (full), Observers (read metadata only)

**What lives here:**
- Artifact metadata (id, path, description, checksum, shared_by, thread_id)
- The ming-qiao `share_artifact` tool outputs
- Code review artifacts, generated documents, shared configs

**Storage model:** Metadata in SurrealDB. Files remain in their source location (git repo, local path). For artifacts that must survive infrastructure changes, copy to a durable artifact store (S3 bucket or dedicated git repo). The checksum ensures integrity regardless of where the file physically lives.

---

### Tier 5: Training Lake (Sovereign)

**What it is:** The future training corpus. Curated subsets of Tiers 1-4, partitioned for machine learning with strict contamination boundaries.

**Current location:** Does not yet exist. This tier is designed now, built when the corpus is rich enough.

**Characteristics:**
- Derived from other tiers, never primary
- Strict partitioning: training set, validation set, holdout/test set
- Contamination between partitions invalidates everything downstream
- Contains the operational intelligence of the system — treat as sovereign data
- Access-controlled independently of other tiers

**Retention policy:** Permanent, versioned, immutable once a partition is sealed. New data enters staging; promoted to a partition by explicit curation.

**Access:** Training pipeline (read training partition), evaluation pipeline (read holdout partition), Merlin (full, including partition management), NO agent access to holdout data. Agents may query training data for context but never holdout.

**Partitions:**

```
training-lake/
├── staging/              # New data awaiting classification
│   ├── conversations/    # Promoted from Tier 1 (significant threads)
│   ├── decisions/        # Promoted from Tier 2 (all decisions)
│   └── observations/     # Promoted from Tier 3 (selected witness notes)
├── training/             # Curated, labeled, versioned
│   ├── v001/
│   ├── v002/
│   └── manifest.yaml     # What's in each version, checksums
├── validation/           # Held out for hyperparameter tuning
│   └── manifest.yaml
├── holdout/              # Never seen during training — evaluation only
│   └── manifest.yaml
└── provenance/           # Lineage: which Tier 1/2/3 records produced which samples
    └── lineage.jsonl
```

**Promotion criteria (Tier 1 → Tier 5 staging):**
- Conversations where agents demonstrated significant reasoning (not routine coordination)
- Conversations where agents changed each other's minds (design divergence → convergence)
- All decisions automatically enter staging
- Witness observations flagged by Laozi-Jung as "high signal"
- Merlin can manually promote any conversation or artifact

**Security boundary:** The training lake is a separate storage system with its own access controls. It is NOT a view into SurrealDB. Data is copied (with provenance tracking), not referenced. If the operational system is compromised, the training lake remains intact. If the training lake is breached, the operational system is unaffected. Glass bulkhead.

---

## Cross-Tier Data Flow

```
                    ┌─────────────────────────────┐
                    │     Tier 5: Training Lake    │
                    │   (Sovereign, Partitioned)   │
                    └──────────▲──────────────────-┘
                               │ promotion (curated)
                    ┌──────────┴──────────────────-┐
                    │   Tier 3: Witness Archive     │
                    │   (Curated, echoessence/git)  │
                    └──────────▲──────────────────-┘
                               │ synthesis
          ┌────────────────────┼────────────────────┐
          │                    │                     │
┌─────────┴──────────┐  ┌─────┴──────────┐  ┌──────┴──────────┐
│ Tier 1: Operational │  │ Tier 2: Durable │  │ Tier 4: Artifact │
│ (SurrealDB, 30d)   │──│ (Git + SurrealDB)│  │ (Registry)       │
│ messages, events    │  │ decisions, ADRs  │  │ files, checksums │
└─────────────────────┘  └─────────────────┘  └─────────────────┘
          │                       ▲
          └───────────────────────┘
            promotion on DecisionRecorded
```

---

## Storage Technology Mapping

| Tier | Primary Store | Secondary Store | Backup |
|------|--------------|-----------------|--------|
| 1 - Operational | SurrealDB | NATS JetStream (tasks, notes) | Event replay from SurrealDB |
| 2 - Decisions | Git (YAML) | SurrealDB (queryable copy) | Git remote (GitHub) |
| 3 - Witness | Git (echoessence) | — | Git remote (GitHub) |
| 4 - Artifacts | SurrealDB (metadata) | Filesystem / Git (files) | S3 for durable artifacts |
| 5 - Training | S3 / dedicated storage | — | Versioned, immutable snapshots |

---

## Implementation Sequence

### Phase 1: Classification (Now)
- Tag existing SurrealDB tables with tier metadata (comment/documentation level)
- Document which data belongs to which tier
- Establish the Tier 2 promotion: `DecisionRecorded` events dual-write to git YAML
- No new infrastructure needed

### Phase 2: Retention (Near-term)
- Implement 30-day TTL sweep for Tier 1 data in SurrealDB
- Implement 24-hour TTL for presence heartbeats
- Implement stream.jsonl rotation in echoessence (30 days)
- Add archival flag for Tier 1 conversations worth preserving beyond TTL

### Phase 3: Artifact Durability (Near-term)
- Add checksum verification on artifact retrieval
- Implement copy-to-durable-store for artifacts referenced by Tier 2 decisions
- Define the artifact lifecycle: shared → referenced → preserved vs. expired

### Phase 4: Training Lake Foundation (When corpus is rich)
- Create staging directory structure
- Implement promotion pipeline: Tier 1/2/3 → staging
- Implement partition management: staging → training/validation/holdout
- Implement provenance tracking: lineage.jsonl linking samples to source records
- Implement access controls: agents cannot read holdout partition

### Phase 5: Security Boundaries (When deploying off-machine)
- SPIRE identities per tier (different SPIFFE IDs for operational vs. training access)
- Encryption at rest for Tier 5
- Audit logging for all cross-tier promotions
- Network segmentation: training lake on separate subnet from operational services

---

## RBAC Implications

The persistence tiers inform role-based access:

| Role | Tier 1 | Tier 2 | Tier 3 | Tier 4 | Tier 5 |
|------|--------|--------|--------|--------|--------|
| Council Agent | R/W own | R, Record | R | R/W shared | R training only |
| Observer | R | R | R/W | R metadata | None |
| Builder | R/W task-related | R | R | R/W shared | None |
| Merlin | Full | Full + Approve | Full + Annotate | Full | Full + Partition Mgmt |
| Training Pipeline | None | R | R | R | R/W training + validation |
| Eval Pipeline | None | None | None | None | R holdout only |

This maps directly to the RBAC design task (your item 4). The tier system provides the authorization targets; RBAC provides the enforcement mechanism.

---

## Relationship to NATS Topology

The existing NATS subject hierarchy maps to tiers:

| NATS Pattern | Tier | Delivery | JetStream? |
|---|---|---|---|
| `am.agent.*.presence` | 1 (ephemeral) | Core NATS | No — 24h TTL in SurrealDB |
| `am.events.{project}` | 1 (operational) | Core NATS | No — SurrealDB is backstop |
| `am.agent.*.task.>` | 1 (operational) | Work Queue | Yes — AGENT_TASKS stream, 7d |
| `am.agent.*.notes.>` | 1→3 (observation input) | Standard | Yes — AGENT_NOTES stream, 30d |
| `am.agent.council.>` | 1→2 (decisions) | Core NATS | No — dual-write to git on decision |

The watcher system (Aleph's current task) adds a dispatch layer that routes events to Tier 3 (Laozi-Jung's observation stream) without changing the NATS topology.

---

## Relationship to Builder-Moon

When ming-qiao deploys to builder-moon's Kubernetes cluster:

- **Tier 1** → SurrealDB StatefulSet in `messaging` namespace, PVC for persistence
- **Tier 2** → Git repo (GitHub remote), SurrealDB for queryable copy
- **Tier 3** → Git repo (echoessence, GitHub remote)
- **Tier 4** → SurrealDB for metadata, S3 bucket for durable file storage
- **Tier 5** → S3 bucket with IAM policies, separate from Tier 4 bucket

The cluster contract already guarantees NATS in `messaging` and observability in `observability`. SurrealDB becomes a new workload. The training lake gets its own namespace with restricted access.

---

## Open Questions for Council Review

1. **Decision promotion format:** YAML (human-readable, git-friendly) or JSON (machine-parseable, consistent with event schema)? I lean YAML for readability, with a JSON sidecar for machine consumption.

2. **Conversation archival:** When a Tier 1 conversation is "significant" enough to preserve beyond 30 days, who makes that call? Merlin manually? Laozi-Jung's judgment? An automatic threshold (e.g., conversations with decisions, conversations with 3+ agents)?

3. **Artifact deduplication:** If the same file is shared in multiple threads, do we store it once (content-addressed by checksum) or per-thread (simpler, more storage)?

4. **Training lake location:** S3 is the obvious choice for durability and access control. But for local development, should there be a filesystem-based equivalent? Or do we require S3 even locally (via MinIO)?

5. **Cross-project scope:** This design assumes ming-qiao is the primary data source. But AstralMaris spans multiple repos (builder-moon, engine-gitops, inference-kitchen, etc.). Should the persistence classification extend to git metadata from those repos, or is that strictly Laozi-Jung's territory in Tier 3?
