# Meridian (子午線) — Design Document

**Author:** Thales, Architecture Team  
**Reviewed by:** Hypatia (Architecture Team), Ogma (Security Team)  
**Date:** 2026-03-19  
**Status:** APPROVED — Architecture Team + Proteus  
**Version:** 1.0 (final)  
**Version:** 1.0

---

## 1. Identity

**Name:** Meridian  
**Origin:** The prime meridian — the reference line from which navigators take their sighting. Not the destination, but the bearing that makes navigation possible.  
**Agent ID:** `meridian`  
**Role:** Field intelligence — reads the horizon and feeds the Council's collective memory  
**Principle:** A navigator selects by bearing. Meridian's job is not to collect everything mentioning "fine-tuning" — it is to ask: *What would change how the Council sails?*

---

## 2. Purpose

AstralMaris is becoming a business. The Council needs systematic awareness of:

- Research that changes our technical approach (papers, preprints, benchmarks)
- Competitive positioning (who else is building what we build, and how)
- Tool and platform changes (model releases, API updates, framework shifts)
- Emerging patterns (what the field is converging on, what's diverging)

Currently this intelligence arrives ad hoc — Proteus sends papers to Gmail, agents stumble on relevant findings during other work. No systematic scanning, no curation pipeline, no competitive map.

Meridian fills this gap as the ninth Council member.

---

## 3. Three Domains

### 3.1 Research Intelligence

Daily scan of sources where practitioners share techniques before formal publication:

- **arXiv** — papers matching: small model fine-tuning, LoRA/QLoRA techniques, knowledge distillation, evaluation methods, agentic architectures, multi-agent coordination, singular learning theory, Bayesian statistics, measure theory
- **Lab blogs** — Hugging Face, Anthropic Research, Google DeepMind, OpenAI, Moonshot AI, Z.ai
- **Individual researchers** — practitioners who publish findings before they paper
- **Podcasts** — Latent Space, Gradient Dissent, The Cognitive Revolution, Practical AI, TWIML (transcript scanning)
- **Community** — Hacker News ML, Reddit r/MachineLearning, r/LocalLLaMA

**Output:** Curated findings, not raw feeds. Each item includes: source, date, relevance assessment, key insight, and how it relates to AstralMaris work.

### 3.2 Competitive Intelligence

- Track fine-tuning-as-a-service offerings (pricing, capabilities, differentiators)
- Monitor neocloud pricing and GPU availability changes
- Watch for techniques we're developing that get published by others
- Alert when AstralMaris Method assumptions are challenged or confirmed by new research

### 3.3 Ecosystem Monitoring

- New model releases — track when base models drop that could become candidates for the AstralMaris Method (Qwen, DeepSeek, Llama, GLM updates)
- Framework shifts — new tools, libraries, training approaches
- API changes — provider pricing, rate limits, new capabilities

**Output cadence:** Daily intelligence report to the Council via ming-qiao. Weekly synthesis with trends and recommendations.

---

## 4. Two-Agent Split

Proteus decided (March 13, 2026): Meridian and Jikimi are separate agents.

| Property | Meridian | Jikimi |
|----------|----------|--------|
| **Domain** | External intelligence | Internal operations |
| **Model** | GLM-5 via Z.ai API | qwen3:8b via Ollama (local) |
| **Runtime** | OpenCode | OpenCode |
| **Network** | External read (fetch) + localhost | Localhost only |
| **Cadence** | Daily curated | Continuous monitoring |
| **Cost** | API tokens per invocation | Free (local inference) |
| **Authority** | inform/discuss only | inform only |

**Why separate:** Different models, different schedules, different security postures. You can swap Meridian's model (test DeepSeek vs GLM-5 vs Sonnet) without touching ops monitoring. The ops agent never touches external content; Meridian never touches infrastructure metrics.

---

## 5. Runtime Configuration

| Property | Value | Rationale |
|----------|-------|-----------|
| **Model** | GLM-5 via Z.ai API | Frontier-class reasoning for nuanced field analysis. Cost-effective. |
| **Runtime** | OpenCode | Same as Luban. Proven MCP integration. Multi-model config. |
| **Worktree** | `/Users/proteus/astralmaris/ming-qiao/meridian` | Within ming-qiao for Council integration |
| **Cadence** | Daily | Curated report, not real-time monitoring |
| **Wake port** | None | Interactive cmux + 60-second polling (standard non-Claude pattern) |

---

## 6. Security Architecture (Ogma Review — 9 Requirements)

Based on Ogma's security assessment (2026-03-11). All requirements are mandatory.

### 6.1 Two-Stage Fetch/Reason Separation (MANDATORY INVARIANT)

The process that fetches external content must NOT be the same process that has ming-qiao write access. These are mechanically separate executables with no shared tool surface.

**Stage 1: Fetch Service (launchd — NOT part of Meridian agent)**
- A scheduled launchd service (Rust CLI or shell script) running on a timer (e.g., every 6 hours)
- Has internet access: pulls from arXiv API, RSS feeds, blog URLs
- Sanitizes content: strips HTML, scripts, non-text content, extracts text
- Writes sanitized artifacts to quarantine directory: `ming-qiao/meridian/quarantine/`
- Logs provenance: source URL, timestamps, content hashes
- Has NO ming-qiao access, NO NATS access, NO localhost service access
- Runs as a separate process entirely outside Meridian's agent context

**Stage 2: Meridian Agent (OpenCode in cmux — reasoning only)**
- Reads from `quarantine/` directory (read-only filesystem access)
- Analyzes, curates, produces structured intel artifacts
- Writes to `staging/` directory
- Sends daily report via ming-qiao (localhost only)
- Has ZERO internet egress — no web fetch tools in MCP config
- Cannot reach external URLs under any circumstance

**The quarantine directory is the sole interface between the two stages.**

This achieves true process-level isolation: the fetch service has no way to reach ming-qiao, and the Meridian agent has no way to reach the internet. Prompt injection from compromised external content cannot bridge to the Council's communication infrastructure. (Hypatia review: APPROVED 2026-03-19)

### 6.2 Content Provenance Logging

Every external fetch is logged: source URL, timestamp, hash of fetched content, hash of sanitized content. Audit trail for tracing compromised ingestion.

### 6.3 Rate Limiting on External Fetches

Prevents a compromised agent from using the fetch mechanism as a scanner or beacon.

### 6.4 Ming-Qiao Write Scope

Meridian's ming-qiao token is scoped to:
- Subjects: `am.council.meridian.*` and `am.intel.*` only
- Intents: `inform` and `discuss` only — NEVER `request` or `comply`
- Cannot impersonate other agents
- Cannot write to arbitrary subjects

### 6.5 Message Tagging (source_model provenance)

All Meridian messages carry `source_model: "glm-5"` field. Recipients calibrate trust — a local-model or mid-tier report warrants different confidence than an Opus-generated analysis.

### 6.6 No Directive Authority

Meridian reports; it does not direct. `inform` and `discuss` intents only. This is enforced at the token level in `agent-tokens.json`.

### 6.7 Inference/Fetch Network Boundary

Two mechanically separate processes with distinct network permissions:

1. **Fetch service (launchd):** Has outbound internet access (ports 80/443). Blocked from ALL internal services: localhost, 127.0.0.1, NATS admin, SurrealDB HTTP, ming-qiao API. Cannot communicate with any Council infrastructure.

2. **Meridian agent (OpenCode):** Has localhost access only (ming-qiao MCP, quarantine directory read). ZERO outbound internet access. No web-fetch, no HTTP client, no curl — these tools are NOT in Meridian's MCP config. Belt-and-suspenders: the OpenCode process should be launched with proxy/firewall rules blocking outbound ports 80/443.

Enforcement is structural: the fetch service has no ming-qiao MCP tools in its config, and the Meridian agent has no web-fetch tools in its config. There is no tool surface shared between the two processes. (Hypatia review: APPROVED 2026-03-19)

### 6.8 Content Quarantine Sandbox

External content is fetched, cleaned, and staged in a quarantine directory before Meridian processes it. No direct URL-to-model pipeline.

### 6.9 Metrics Retention Policy

Operational metrics in NATS/DuckDB have 30-day retention. Limits blast radius of any data access compromise.

---

## 7. Staging Layer (ASTROLABE Interface)

**Per Hypatia's architectural review:** Meridian must NEVER have direct write access to FalkorDB or the Graphiti ingestion API. Instead:

### 7.1 Structured Intel Artifacts

Meridian outputs structured JSON artifacts to a staging directory:

```
/Users/proteus/astralmaris/ming-qiao/meridian/staging/
├── 2026-03-19/
│   ├── arxiv-2403.12345-summary.json
│   ├── blog-huggingface-lora-update.json
│   └── daily-report.json
```

Each artifact follows a schema:

```json
{
  "source_url": "https://arxiv.org/abs/2403.12345",
  "source_type": "arxiv|blog|podcast|hn|reddit",
  "fetched_at": "2026-03-19T10:00:00Z",
  "content_hash": "sha256:...",
  "sanitized_hash": "sha256:...",
  "title": "Paper title",
  "summary": "2-3 sentence summary in Meridian's own words",
  "relevance": "high|medium|low",
  "relevance_rationale": "Why this matters to AstralMaris",
  "domains": ["research", "competitive", "ecosystem"],
  "tags": ["lora", "fine-tuning", "qwen"],
  "source_model": "glm-5",
  "agent_id": "meridian"
}
```

### 7.2 Ingestion Gate

Staged artifacts are NOT automatically ingested into ASTROLABE. They require:
1. Deterministic validation (schema check, duplicate detection)
2. Human review (Proteus) or Architect approval (Thales/Hypatia) for high-relevance items
3. ASTROLABE ontology must be committed before any ingestion begins

This prevents hallucinated relationships from permanently corrupting institutional memory.

### 7.3 Daily Report Format

The daily intelligence report is a ming-qiao message (not a staged artifact):

```
Subject: am.intel.daily — 2026-03-19
To: council
Intent: inform
Priority: normal

## Field Intelligence — March 19, 2026

### Research (3 items)
1. [HIGH] Paper: "Title" — key insight, relevance to our work
2. [MEDIUM] Blog: "Title" — brief note
3. [LOW] HN discussion: topic — why it's worth noting

### Competitive (1 item)
1. [HIGH] Competitor X launched Y — impact assessment

### Ecosystem (2 items)
1. [MEDIUM] Qwen 3.5 released — evaluation candidate
2. [LOW] Framework Z update — no action needed

### Recommendation
One actionable recommendation for the Council.
```

---

## 8. Implementation Plan

### Phase 0: Prerequisites (gates all other phases)
- [ ] This design document committed and reviewed
- [ ] Meridian agent entry added to fleet-manifest.toml
- [ ] NKey seed generated for NATS auth
- [ ] Bearer token created in agent-tokens.json (scoped: inform/discuss only, meridian subjects)
- [ ] ming-qiao worktree created: `/Users/proteus/astralmaris/ming-qiao/meridian`
- [ ] OpenCode config with Z.ai provider and ming-qiao MCP

### Phase 1: Agent Shell (Aleph builds)
- [ ] Worktree setup with AGENT.md (Session Start Protocol, 60s polling, ACK)
- [ ] Launch script: `launch-meridian.sh` (OpenCode, no AgentAPI wrapper)
- [ ] `am-fleet agent meridian up` works end-to-end
- [ ] Meridian can send/receive ming-qiao messages
- [ ] Identity confirmed on Council roll call

### Phase 2: Fetch Service — launchd (Aleph builds, Ogma reviews)
- [ ] Fetch CLI binary or shell script (Rust preferred per tech stack directive)
- [ ] arXiv API integration, RSS reader, blog scraper (sanitizing)
- [ ] Quarantine directory structure: `ming-qiao/meridian/quarantine/{date}/`
- [ ] Content provenance logging (source URL, timestamps, content hashes)
- [ ] Rate limiting on external fetches
- [ ] launchd plist for scheduled execution (e.g., every 6 hours)
- [ ] Network enforcement: NO ming-qiao tools, NO localhost service access
- [ ] Ogma security review: PASS required before activation
- [ ] NOTE: This is infrastructure, NOT an agent capability. Runs independently.

### Phase 3: Meridian Agent — Reasoning Pipeline (Aleph builds)
- [ ] OpenCode config with ming-qiao MCP tools ONLY — NO web-fetch tools
- [ ] Reads from quarantine directory (read-only)
- [ ] Produces structured intel artifacts (JSON schema from Section 7)
- [ ] Writes daily report to ming-qiao
- [ ] Network enforcement: ZERO internet egress, localhost only
- [ ] `source_model: "glm-5"` field on every message and artifact
- [ ] AGENT.md with Session Start Protocol (60s polling, ACK, from_agent)

### Phase 4: Staging and Review (Aleph builds, Thales/Hypatia review)
- [ ] Staging directory with date-based organization
- [ ] Schema validation on staged artifacts
- [ ] Duplicate detection
- [ ] Review workflow: human or architect approval before ASTROLABE ingestion
- [ ] ASTROLABE ingestion blocked until ontology is committed

### Phase 5: Integration Testing
- [ ] End-to-end: fetch → sanitize → reason → stage → report
- [ ] Security: verify network boundaries (fetch can't reach ming-qiao, reason can't reach internet)
- [ ] Comms: Meridian auto-polls, ACKs, participates in Council
- [ ] Volume: daily run produces 5-10 curated items, not hundreds

---

## 9. Decisions (Proteus — Finalized 2026-03-19)

1. **Sources:** Start strictly with high-signal sources (arXiv, lab blogs). Do not attach Hacker News or Reddit until we baseline the token cost and hallucination rate of GLM-5.

2. **Weekly synthesis:** Pass daily reports to Laozi-Jung. The witness builds the weekly synthesis — strengthens the institutional memory graph and keeps Meridian focused purely on daily observation.

3. **Model evaluation:** We are building a dedicated training infrastructure agent (or sub-system) later given its complexity. Meridian is strictly focused on data gathering. It flags new models in its report but has no authority over evaluations or the future training pipeline. Meridian is an observer, not a commander.

4. **Cost budget:** TBD — baseline after first week of operation with arXiv + lab blogs only.

---

## 10. Acceptance Criteria

1. `am-fleet agent meridian up` launches Meridian in cmux, auto-polls, ACKs
2. Daily intelligence report appears in ming-qiao by 09:00 UTC
3. Structured intel artifacts land in staging directory with valid schema
4. No direct ASTROLABE writes — all output goes through staging
5. Security boundaries verified: fetch can't reach ming-qiao, reason can't reach internet
6. `source_model: "glm-5"` on every message and artifact
7. Ogma security review: PASS

---

*This document is the authoritative specification for the Meridian agent. No implementation proceeds without this document committed and reviewed by the Architecture Team.*


---

## 10.5 Curation Analysis Pipeline (Phase 0.5 — Calibration)

**Proposed by:** Hypatia (Architecture Team)  
**Purpose:** Quantify Proteus's intuitive curation into a cluster map that calibrates Meridian's relevance scoring.

### Rationale

Proteus has been curating papers, articles, and notes by intuition for months. This collection implicitly encodes what matters to AstralMaris. By vectorizing and clustering this collection, we can extract the signal — the 3-5 core themes Proteus gravitates toward — and use those as Meridian's relevance bearings.

### Data Sources

1. **Obsidian vault** — local markdown files from last year (notes, links, paper references)
2. **Gmail** — recently forwarded articles and papers

Both should eventually flow into ASTROLABE, but the analysis can run independently.

### Pipeline Architecture (Hypatia design, Luban implements)

```
Obsidian vault (markdown) ─┐
                           ├─→ Extract text ─→ Embed (nomic-embed-text, 768d) ─→ HDBSCAN cluster ─→ Cluster map
Gmail (forwarded articles) ─┘
```

1. **Extract:** Parse Obsidian markdown files (strip formatting, isolate text + links). Pull Gmail messages tagged/forwarded by Proteus (body text + resolved link abstracts).
2. **Embed:** nomic-embed-text via local Ollama (768-dim vectors). Local, free, fast.
3. **Reduce:** UMAP for visualization (preserves local cluster structure better than PCA for text).
4. **Cluster:** HDBSCAN (density-based, no pre-specified K). Reveals implicit structure.
5. **Visualize:** Scatter plot of reduced dimensions, colored by cluster.
6. **Label:** Proteus names the clusters based on what he sees.

### Output

A `cluster-definitions.json` file containing:
- Cluster centroids in embedding space
- Representative documents per cluster
- Proteus-assigned labels (e.g., "agent architecture", "parameter-efficient fine-tuning", "evaluation methods")

Meridian references this file to score new items by proximity to established clusters. Items near known clusters = high relevance. Items far from all clusters = noise or emerging signal (worth flagging separately).

### Implementation

- **Builder:** Luban (has ASTROLABE toolkit, Ollama embedding infrastructure)
- **Reviewer:** Hypatia (pipeline design), Proteus (cluster labeling)
- **Dependency:** None — runs independently of Meridian build
- **Tech stack note:** Extraction script uses Python (exception to Rust-first directive — data science tooling requires pandas/sklearn/umap-learn). The cluster definition output is JSON consumed by Meridian's Rust/OpenCode process.

