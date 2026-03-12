# Meridian Ingest Envelope Schema

**Author:** Luban
**Date:** 2026-03-12
**Thread:** RE: PROPOSAL: Meridian — Envelope spec + query patterns
**Consumers:** Mataya (quarantine-as-curation UI), Ogma (security audit surfaces), Meridian (field intelligence pipeline)

---

## Overview

The ingest envelope is the JSON contract between Meridian (content fetcher/curator) and ASTROLABE (knowledge graph). Every item Meridian selects for ingestion passes through this envelope, which serves three purposes simultaneously:

1. **Pipeline contract** — tells astrolabe-ingest.py what to do
2. **Quarantine surface** — gives mataya's UI the data it needs for the curation view
3. **Audit trail** — gives Ogma visibility into what enters the knowledge graph

---

## Full Envelope Schema

```json
{
  "name": "lora-rank-selection-2026",
  "source": "text",
  "source_type": "arxiv_paper",
  "source_url": "https://arxiv.org/abs/2603.12345",
  "source_description": "arXiv 2603.12345 — Li et al., March 2026",
  "content": "This paper demonstrates that adaptive LoRA rank selection based on layer-wise gradient norms improves fine-tuning efficiency by 23% on instruction-following benchmarks...",
  "domain_tags": ["lora", "fine-tuning", "evaluation"],
  "relevance_note": "Directly applicable to AstralMaris Method — our current fixed-rank approach may be leaving performance on the table.",
  "relevance_score": 0.87,
  "source_agent": "meridian",
  "enrichment_status": "quarantined",
  "rejection_reason": null,
  "fetched_at": "2026-03-12T08:30:00Z",
  "ingested_at": null,
  "meridian_model": "qwen3:14b",
  "pre_sanitization_hash": "sha256:a1b2c3...",
  "post_sanitization_hash": "sha256:d4e5f6..."
}
```

---

## Field Reference

### Pipeline fields (ASTROLABE ingestion)

These fields map directly to `add_memory` MCP tool parameters.

| Field | Type | Required | ASTROLABE param | Description |
|-------|------|----------|-----------------|-------------|
| `name` | string | yes | `name` | Descriptive slug for the episode. Should be human-readable and unique within a batch. |
| `source` | enum | yes | `source` | Content format for Graphiti's parser. One of: `"text"`, `"json"`, `"message"`. Most field intel uses `"text"`. |
| `source_description` | string | yes | `source_description` | Free-text provenance string. Include author, date, and identifier (DOI, URL slug, episode number). |
| `content` | string | yes | `episode_body` | The actual content to ingest. See [Content Guidelines](#content-guidelines) per source type. |

### Metadata fields (quarantine UI + audit)

These fields are NOT passed to ASTROLABE directly. They exist in the envelope for UI display, audit, and pipeline orchestration.

| Field | Type | Required | Visibility | Description |
|-------|------|----------|------------|-------------|
| `source_type` | enum | yes | UI + audit | Content category. See [Source Types](#source-types). |
| `source_url` | string | recommended | **audit only** | Original URL. Write-once. Not exposed to UI directly — Ogma audits the full provenance chain; mataya sees curated state only. |
| `domain_tags` | string[] | yes | UI + audit | Which AstralMaris concern areas this item touches. See [Domain Tags](#domain-tags). |
| `relevance_note` | string | yes | UI | Why Meridian selected this item. One sentence explaining relevance to AstralMaris work. This is the curatorial signal Laozi-Jung identified as essential. |
| `relevance_score` | float | yes | UI + audit | Meridian's confidence in relevance, 0.0–1.0. Drives threshold enforcement: >0.8 auto-ingest, 0.5–0.8 queue for review, <0.5 discard. See [Relevance Threshold Enforcement](#relevance-threshold-enforcement). |
| `source_agent` | string | yes | **audit only** | Which agent produced this envelope. Write-once. Enables canary spot-checks by querying ASTROLABE episodes by source_agent. |
| `rejection_reason` | string \| null | no | UI + audit | Why this envelope was rejected. Write-once — populated only when `enrichment_status` transitions to `rejected`. Null for all other statuses. Audit trail for exclusions is as important as audit trail for inclusions. |
| `enrichment_status` | enum | yes | UI + audit | Pipeline stage. See [Enrichment Status](#enrichment-status). |
| `fetched_at` | ISO 8601 | yes | UI + audit | When Meridian fetched/discovered the content. |
| `ingested_at` | ISO 8601 \| null | no | UI + audit | When ASTROLABE ingestion completed. Null while quarantined or in progress. |
| `meridian_model` | string | no | audit | Which local model produced the distillation and relevance assessment. For tracking model quality. |
| `pre_sanitization_hash` | string | yes | **audit only** | SHA-256 of the raw fetched content before any processing. Write-once. Never exposed to UI. |
| `post_sanitization_hash` | string | yes | audit | SHA-256 of the distilled/sanitized content (the `content` field). Deduplication key. |

---

## Source Types

```
arxiv_paper      — arXiv preprints (abstract + key contributions)
blog_post        — Lab/practitioner blog posts (full text, cleaned)
podcast_transcript — Summarized segments from relevant podcasts
hn_thread        — Hacker News discussion summaries
reddit_thread    — Reddit ML discussion summaries
model_release    — New model announcements (capabilities, benchmarks, availability)
tool_release     — New tool/library announcements
pricing_update   — Provider pricing or availability changes
```

---

## Domain Tags

Controlled vocabulary. Meridian should tag with one or more:

```
fine-tuning          — Fine-tuning techniques, methods, tooling
lora                 — LoRA, QLoRA, adapter methods
knowledge-distillation — Knowledge distillation, model compression
evaluation           — Benchmarks, evals, metrics
agentic-architecture — Agent design, tool use, planning
multi-agent          — Multi-agent coordination, communication
slt                  — Singular learning theory, RLCT, Bayesian
base-model           — New base model releases, capabilities
infrastructure       — Training infra, serving, deployment
competitive          — Competitor analysis, market positioning
```

New tags may be added as the field evolves; the list is not closed. But prefer existing tags before creating new ones to keep the graph navigable.

---

## Immutability Constraints

Per Ogma's security requirements, envelope fields follow strict mutability rules:

**Write-once fields (immutable after creation):**
- `name`, `source`, `source_type`, `source_url`, `source_description`
- `content` (the distilled text — set once during sanitization)
- `domain_tags`, `relevance_note`, `relevance_score`
- `source_agent`
- `fetched_at`
- `pre_sanitization_hash`, `post_sanitization_hash`
- `meridian_model`
- `rejection_reason` (set once on `rejected` transition — null until then)

**Forward-only fields (transitions are monotonic, never backward):**
- `enrichment_status` — valid transitions: `quarantined → ingesting → complete | failed | rejected`. No backward transitions. A `failed` item may be retried by creating a new envelope, not by resetting the original.
- `ingested_at` — set once when status reaches `complete`. Never cleared.

**Mandatory audit fields:** `source_url`, `source_agent`, `relevance_score`, `fetched_at`, `pre_sanitization_hash`, `post_sanitization_hash`, `enrichment_status`. These MUST be present on every envelope. An envelope missing any of these is invalid and must not enter the pipeline. See [Envelope Validation](#envelope-validation).

This is append-only by design, not convention. Implementations should enforce these constraints at the type level (e.g., Rust newtypes, frozen dataclasses) rather than relying on documentation alone.

---

## Field Visibility Summary

| Visible to | Fields |
|------------|--------|
| **Mataya (UI)** | `name`, `source_type`, `domain_tags`, `relevance_note`, `relevance_score`, `enrichment_status`, `rejection_reason`, `fetched_at`, `ingested_at`, `post_sanitization_hash`, `content` (post-sanitization only) |
| **Ogma (audit)** | All fields — full provenance chain including `source_url`, `source_agent`, `pre_sanitization_hash`, raw metadata |
| **ASTROLABE (ingestion)** | `name`, `source`, `source_description`, `content` only (see [Mapping to add_memory](#mapping-to-astrolabe-add_memory-call)) |

---

## Enrichment Status

The `enrichment_status` field tracks where an item is in the pipeline. This is the field mataya's UI uses to distinguish what's pending from what's resolved into the graph.

| Status | Meaning | UI treatment |
|--------|---------|--------------|
| `quarantined` | Fetched by Meridian, awaiting ingestion. Content is in the staging area but not yet in ASTROLABE. | Show in quarantine/pending view. Ogma can inspect. |
| `ingesting` | Currently being processed by astrolabe-ingest.py / Graphiti entity resolver. | Show with spinner or "processing" indicator. |
| `complete` | Successfully ingested into ASTROLABE. Entities and facts extracted. | Show as resolved. Link to graph entities if available. |
| `failed` | Ingestion failed (timeout, NodeResolutions error, etc.). | Show with error state. Include failure reason for debugging. |
| `rejected` | Rejected from quarantine — either by relevance threshold (<0.5) or manual Council curation. `rejection_reason` field contains the cause. | Show as struck-through or filtered out. Display `rejection_reason`. |

The temporal gap between `quarantined` and `complete` (~11 min/item on qwen3:14b) is itself a signal about pipeline health. The UI should make this visible, not hide it.

---

## Relevance Threshold Enforcement

The `relevance_score` field drives automated routing decisions at the quarantine boundary. Meridian assigns a score from 0.0 to 1.0 when it evaluates fetched content against AstralMaris concern areas.

| Score range | Action | Rationale |
|-------------|--------|-----------|
| **> 0.8** | Auto-ingest | High confidence. Envelope enters quarantine with `enrichment_status: "quarantined"` and proceeds to ingestion without manual review. |
| **0.5 – 0.8** | Queue for review | Moderate confidence. Envelope enters quarantine but is flagged for canary reviewer attention before ingestion proceeds. |
| **< 0.5** | Discard | Low confidence. Envelope is written with `enrichment_status: "rejected"` and `rejection_reason` populated with the model's assessment. Never enters the ingestion queue. |

These thresholds are security-relevant content filtering, not convenience. They bound what enters the knowledge graph autonomously vs. what requires human judgment. Threshold values are configurable but changes require Council discussion.

---

## Envelope Validation

The scheduler validates every envelope before transitioning it from `quarantined` to `ingesting`. This is the enforcement point for the "no action bypasses the pipeline" constraint.

### Mandatory field check

The following fields MUST be present and non-null on every envelope:

- `name`
- `source`
- `source_type`
- `source_url`
- `source_description`
- `content`
- `domain_tags` (non-empty array)
- `relevance_note`
- `relevance_score` (float, 0.0–1.0)
- `source_agent`
- `fetched_at` (valid ISO 8601)
- `pre_sanitization_hash`
- `post_sanitization_hash`
- `enrichment_status`

### Validation behavior

- **Valid envelope:** Proceeds to `ingesting` status normally.
- **Invalid envelope:** Transitions to `enrichment_status: "failed"` with a specific validation error describing which field was missing or malformed. No silent drops — every rejection is recorded with cause.
- **Validation error format:** The error must identify the specific field(s) that failed validation. Generic "validation failed" messages create debugging blind spots. Example: `"Validation failed: missing mandatory field 'source_agent'; 'relevance_score' is not a valid float"`.

### When validation runs

Validation is a gate at the `quarantined → ingesting` transition. It runs synchronously in the scheduler before any ASTROLABE API call is made. An envelope that fails validation never touches ASTROLABE.

---

## Content Guidelines

What Meridian should put in the `content` field varies by source type. The graph works best with focused, claim-dense text.

| Source type | Content format | What to include | What to exclude |
|-------------|---------------|-----------------|-----------------|
| `arxiv_paper` | Pre-distilled summary | Abstract + key contributions + main claims. One paragraph distillation, not verbatim abstract copy. | Full paper text. Boilerplate. Related work section. |
| `blog_post` | Cleaned full text | Technical content, claims, code examples. | Navigation, headers/footers, ads, author bios. |
| `podcast_transcript` | Summarized segments | Key technical claims, technique descriptions, practitioner insights relevant to our domains. | Introductions, ads, off-topic discussion, raw verbatim transcript. |
| `hn_thread` / `reddit_thread` | Thread summary | Top technical claims, linked resources, consensus/debate positions. | Raw comments, low-signal replies, off-topic tangents. |
| `model_release` | Structured summary | Model name, parameter count, architecture, benchmark results, availability, licensing. | Marketing copy. |
| `pricing_update` | Structured summary | Provider, old price, new price, effective date, what changed. | Marketing copy. |

**Key principle:** Meridian pre-distills before handing to ASTROLABE. The entity resolver produces cleaner entities from focused, claim-dense text than from raw dumps. A 2-hour podcast transcript would overwhelm the entity resolver; 3 paragraphs of key insights won't.

---

## Pipeline Flow

```
[External source] → Meridian fetches → Meridian distills + tags
                                            ↓
                              Envelope written to quarantine dir
                              enrichment_status: "quarantined"
                                            ↓
                              (UI: visible in quarantine view)
                              (Audit: inspectable by Ogma)
                                            ↓
                              Scheduler picks up envelope
                              Envelope validation (mandatory fields check)
                                            ↓
                              Valid → enrichment_status: "ingesting"
                              Invalid → enrichment_status: "failed" (with specific error)
                                            ↓
                              astrolabe-ingest.py → add_memory MCP call
                              (only pipeline fields sent to ASTROLABE)
                                            ↓
                              enrichment_status: "complete" | "failed"
                              ingested_at: timestamp (if complete)
```

**Security constraint (Ogma):** The quarantine view is read-only from the UI. No action from the dashboard bypasses the fetch-sanitize-stage-reason pipeline. The quarantine boundary remains structural even when it becomes visible.

---

## Mapping to ASTROLABE add_memory Call

When an envelope moves from `quarantined` to `ingesting`, only the pipeline fields are sent:

```python
call_tool(session_id, "add_memory", {
    "name": envelope["name"],
    "episode_body": envelope["content"],
    "group_id": "astrolabe_main",
    "source": envelope["source"],               # "text", "json", or "message"
    "source_description": envelope["source_description"],
})
```

The metadata fields (`domain_tags`, `relevance_note`, `enrichment_status`, etc.) remain in the envelope file for UI and audit purposes. They do not enter the graph directly — the entity resolver extracts its own structure from the content.

---

## Open Questions

1. ~~**Envelope storage format:**~~ **Resolved.** JSON files in a quarantine directory. Simpler audit surface than SQLite. (Decision: Council thread 2026-03-12)
2. **Enrichment_status persistence:** Who updates the status field — the ingest scheduler, or a callback from the pipeline? If the pipeline crashes mid-ingestion, how does the status recover? **Partial:** Watchdog with 30-minute timeout recovers stuck `ingesting` items (Decision: Council thread 2026-03-12).
3. ~~**Post-sanitization hash deduplication:**~~ **Resolved.** Meridian deduplicates at the boundary using `post_sanitization_hash`. Items whose hash matches an already-ingested envelope are skipped. `pre_sanitization_hash` is NOT used for dedup. (Decision: Council thread 2026-03-12)
