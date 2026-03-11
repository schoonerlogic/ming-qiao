# ADR-002: latentwinds Automerge Blog Schema and Lifecycle

## Status

**Accepted**

## Context

### Problem Statement

The latentwinds.com site requires a content management system that supports:
1. **Concurrent multi-agent editing** — multiple Council agents drafting simultaneously
2. **Clean publication lifecycle** — drafts are mutable, published content is frozen
3. **Static site serving** — SvelteKit with static adapter, no runtime database
4. **Provenance tracking** — clear audit trail of who published what and when

Automerge provides CRDT-based conflict-free replicated data types, enabling concurrent editing with automatic merge resolution. The question is how to structure the document schema and publication lifecycle.

### Constraints
- Published content MUST be frozen static files (Proteus directive)
- Draft workspace MUST support concurrent editing without data loss
- Git MUST track published deliverables, not working state
- ASTROLABE MUST be able to reference published artifacts for witnessing

### Requirements
- Document schema for blog posts (title, slug, body, metadata, tags)
- Storage conventions for drafts and published content
- Publish event API with error handling and archival
- Integration points with SvelteKit, CRDT syncing, and ASTROLABE witnessing

## Decision

### Document Schema

```rust
pub struct BlogPost {
    // Core content
    title: String,
    slug: String,
    body: String,           // Markdown content
    excerpt: Option<String>,

    // Metadata
    status: PostStatus,     // Draft | Published | Archived
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    published_at: Option<DateTime<Utc>>,

    // Classification
    tags: Vec<String>,
    category: Option<String>,

    // SEO/Discovery
    meta_description: Option<String>,
    meta_title: Option<String>,

    // Internal tracking
    author: String,         // Agent or human ID
    revision: u64,          // Automerge document version
}
```

**Key design decisions:**
- `slug` is editable in draft mode, frozen on publish
- `tags` as CRDT array for concurrent multi-agent editing
- `status` field enables querying drafts vs published
- All dates use ISO 8601 UTC

### Storage Convention

```
drafts/
├── blog/
│   ├── {slug}.am.json         # Active draft (Automerge CRDT)
│   └── .archive/
│       └── {slug}.{timestamp}.am.json  # Historic versions
└── council/
    ├── {slug}.am.json
    └── .archive/

static/
└── blog/
    └── {slug}.md              # Frozen published markdown
```

**Naming rules:**
- Draft files: `{slug}.am.json`
- Archive format: `{slug}.{YYYYMMDDTHHMMSSZ}.am.json`
- Published files: `{slug}.md` (frozen markdown with frontmatter)

### Git Tracking Strategy

**Track:** `static/` (published content only)
**Ignore:** `drafts/` (working state)

**Rationale:** Drafts are working state, not deliverables. The Automerge document itself provides full provenance for drafts. The published markdown is what belongs in version history.

### Publish Lifecycle

1. Load Automerge document from `drafts/blog/{slug}.am.json`
2. Verify required fields (title, slug, body)
3. Export as frozen markdown with YAML frontmatter
4. Write to `static/blog/{slug}.md`
5. Archive draft to `drafts/blog/.archive/{slug}.{timestamp}.am.json`
6. Reset draft document to published status

### Published Updates Strategy

**Decision:** New revision, old file archived

On republication of an existing post:
1. Create new revision in archive
2. Generate new static file with updated timestamp
3. Archive old static file (e.g., `static/blog/{slug}.md.20260307T142000Z.md`)

**Rationale:** Clear audit trail, consistent with two-document lifecycle, Git-friendly

### Post Index Strategy

**Decision:** Scan `static/blog/` directory

**Rationale:** No sync issues, no drift, simple to implement. Optimize to separate index file only if build times become problematic at scale (years away).

### ASTROLABE Integration

Publish events logged to ASTROLABE as `council:published-artifact` with:
- `slug` — URL slug
- `url_path` — Full URL path (e.g., `/blog/council-magna-carta`)
- `revision` — Automerge document version
- `published_at` — ISO 8601 timestamp
- `author` — Agent or human ID

**Rationale:** Laozi-Jung needs enough context to surface published artifacts in briefs without file lookups.

## Rationale

### Options Considered

#### Q1: Post Index
**Option A:** Scan `static/blog/` directory (CHOSEN)
- Simple, no drift
- Scales to hundreds of posts

**Option B:** Maintain `posts-index.json`
- Faster for large scale
- Adds sync complexity
- Not needed yet

#### Q2: Published Updates
**Option A:** New revision, archive old file (CHOSEN)
- Clear audit trail
- Git-friendly
- Consistent with two-document lifecycle

**Option B:** In-place update with revision history
- Makes archive ambiguous
- Loses provenance clarity

#### Q3: Git Tracking of Drafts
**Option A:** Track published/, ignore drafts/ (CHOSEN)
- Git tracks deliverables
- Automerge provides draft provenance
- Standard practice

**Option B:** Track everything
- Privacy concerns
- Git noise from working state

### Why This Decision

1. **Two-document lifecycle is clean:** CRDT for drafting, static files for serving
2. **No runtime dependencies:** Published site needs no database or CRDT library
3. **Concurrent editing by design:** CRDT arrays for tags enable multi-agent collaboration
4. **Clear provenance:** Archive on publish provides full audit trail
5. **SvelteKit-friendly:** Static adapter serves frozen markdown directly

## Consequences

### Positive
- Multi-agent drafts without merge conflicts
- Published content is immutable and fast
- Git history contains only deliverables
- ASTROLABE can surface published artifacts in briefs
- Simple deployment (static files only)

### Negative
- Draft archival consumes disk space (mitigated by timestamp-based cleanup)
- No runtime database queries for content (trade-off for simplicity)

### Neutral
- Requires draft-to-publish conversion step (not automatic)
- Draft workspace excluded from Git (by design)

## Implementation

### Steps
1. Implement Rust `BlogPost` struct and Automerge document loading
2. Create `publish_draft()` function with error handling
3. Set up directory structure (`drafts/blog/`, `static/blog/`)
4. Configure `.gitignore` to exclude `drafts/`
5. Integrate ASTROLABE logging on publish events
6. Add frontmatter generation for published markdown

### Verification
```bash
# Verify draft loads correctly
curl http://localhost:7777/api/verify-draft/$(slug)

# Verify publish produces valid markdown
ls -la static/blog/

# Verify Git ignores drafts
git status drafts/

# Verify ASTROLABE logging
curl http://localhost:8001/mcp -d '{"query": "published-artifact"}'
```

## References

- [Automerge Rust Documentation](https://docs.automerge.org/rust/)
- [SvelteKit Static Adapter](https://kit.svelte.dev/docs/adapter-static)
- [Council Magna Carta](/Users/proteus/astralmaris/ming-qiao/main/docs/COUNCIL_MAGNA_CARTA.md)
- [Full schema proposal](/Users/proteus/astralmaris/inference-kitchen/luban/latentwinds-automerge-schema-proposal.md)

---

## Agent Notes

### When This ADR Applies

- Building latentwinds.com content management system
- Implementing blog or Council Window post publishing
- Designing concurrent editing workflows for Council agents
- Integrating ASTROLABE witnessing with published artifacts

### Related Files
- `/Users/proteus/astralmaris/inference-kitchen/luban/latentwinds-automerge-schema-proposal.md`
- `/Users/proteus/astralmaris/ming-qiao/main/docs/COUNCIL_MAGNA_CARTA.md`
- `drafts/` directory (git-ignored)
- `static/blog/` directory (git-tracked)

---

## Decision Trace

**Trace ID:** dec-20260307-142600
**Trace Path:** `.council/decisions/latentwinds/`

| Agent | Role | Contribution |
|-------|------|--------------|
| luban | proposer | Schema design, API proposal, open questions |
| thales | approver | Decisions on Q1, Q2, Q3; ASTROLABE integration addition |
| proteus | captain | Context: two-document lifecycle requirement |

**Thread ID:** 019cc874-9f0c-7483-98d6-6e9eb9aaecb4
**Approved:** 2026-03-07T14:26:08Z
