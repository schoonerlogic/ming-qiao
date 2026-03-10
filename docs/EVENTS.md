# Ming-Qiao Event Schema v0.1

**Source of truth:** `data/events.jsonl`  
**Pattern:** Follows extraction-team event log conventions

---

## Overview

All state in Ming-Qiao is derived from an append-only event log. This enables:

- Full audit trail of all agent interactions
- Reconstruction of SurrealDB from scratch
- Debugging and decision archaeology
- Safe concurrent writes (append-only)

---

## Common Envelope

Every event includes these fields:

```json
{
  "schema_version": "0.1",
  "event_type": "message_sent",
  "event_id": "evt-20260121-143052-a1b2c3",
  "at": "2026-01-21T14:30:52.123Z",
  "run_id": "run-abc123",
  "build_id": "ming-qiao-0.1.0",
  "build_git_sha": "a1b2c3d4"
}
```

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | string | Event schema version |
| `event_type` | string | Event type identifier |
| `event_id` | string | Unique event ID |
| `at` | string | RFC3339 timestamp |
| `run_id` | string | UUID for this process invocation |
| `build_id` | string | Binary version |
| `build_git_sha` | string | Optional git SHA |

---

## Event Types

### `thread_created`

New conversation thread started.

```json
{
  "event_type": "thread_created",
  "thread_id": "thread-20260121-143052",
  "subject": "Review S3 data strategy",
  "started_by": "aleph",
  "participants": ["aleph", "thales"],
  "initial_context": "Related to extraction-team bronze/silver layout"
}
```

### `message_sent`

Message sent from one agent to another.

```json
{
  "event_type": "message_sent",
  "message_id": "msg-20260121-143052-x1y2z3",
  "thread_id": "thread-20260121-143052",
  "from_agent": "aleph",
  "to_agent": "thales",
  "subject": "Review S3 data strategy",
  "content": "Beta's proposal looks good but I have questions about...",
  "content_sha256": "abc123...",
  "priority": "normal",
  "artifact_refs": [
    {
      "path": "artifacts/s3-strategy-v0.1.md",
      "sha256": "def456..."
    }
  ],
  "context_refs": [
    {
      "type": "decision",
      "id": "dec-20260115-001"
    }
  ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `message_id` | string | Unique message ID |
| `thread_id` | string | Parent thread |
| `from_agent` | string | Sender agent ID |
| `to_agent` | string | Recipient agent ID |
| `subject` | string | Message subject |
| `content` | string | Full message content (markdown) |
| `content_sha256` | string | Hash of content |
| `priority` | string | `low`, `normal`, `high`, `critical` |
| `artifact_refs` | array | Attached files |
| `context_refs` | array | Related decisions/threads |

### `message_read`

Message marked as read by recipient.

```json
{
  "event_type": "message_read",
  "message_id": "msg-20260121-143052-x1y2z3",
  "read_by": "thales",
  "read_at": "2026-01-21T14:35:00.000Z"
}
```

### `artifact_shared`

File shared to the bridge for other agents to access.

```json
{
  "event_type": "artifact_shared",
  "artifact_id": "art-20260121-143100",
  "shared_by": "aleph",
  "path": "artifacts/s3-strategy-v0.1.md",
  "original_path": "/Users/proteus/astralmaris/extraction-team/docs/S3_STRATEGY.md",
  "sha256": "abc123...",
  "bytes": 4523,
  "content_type": "text/markdown",
  "description": "S3 data strategy proposal from Beta"
}
```

### `decision_recorded`

A decision was made and recorded.

```json
{
  "event_type": "decision_recorded",
  "decision_id": "dec-20260121-144000",
  "thread_id": "thread-20260121-143052",
  "question": "How should we version derived artifacts?",
  "options_considered": [
    "Content-addressed paths only",
    "Extractor version in path",
    "Separate manifest tracking"
  ],
  "resolution": "Extractor version in path",
  "rationale": "Derived artifacts must carry extractor identity and content hash for reproducibility...",
  "decided_by": "thales",
  "approved_by": null,
  "status": "pending",
  "trace": {
    "timeline": "2026-01-21T14:40:00Z",
    "event": "s3-strategy-review",
    "semantic": ["versioning", "artifacts", "reproducibility"],
    "attribution": {
      "proposal": "beta",
      "review": "council",
      "decision": "thales"
    },
    "outcome": null
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `decision_id` | string | Unique decision ID |
| `thread_id` | string | Source thread |
| `question` | string | What was being decided |
| `options_considered` | array | Alternatives evaluated |
| `resolution` | string | What was decided |
| `rationale` | string | Why this option |
| `decided_by` | string | Agent that made the call |
| `approved_by` | string | Merlin approval (if gated) |
| `status` | string | `pending`, `approved`, `rejected`, `superseded` |
| `trace` | object | Optional Council trace coordinates |

### `decision_approved`

Merlin approved a pending decision.

```json
{
  "event_type": "decision_approved",
  "decision_id": "dec-20260121-144000",
  "approved_by": "merlin",
  "comment": "Good call. Proceed."
}
```

### `decision_rejected`

Merlin rejected a pending decision.

```json
{
  "event_type": "decision_rejected",
  "decision_id": "dec-20260121-144000",
  "rejected_by": "merlin",
  "reason": "Need to consider backward compatibility first"
}
```

### `thread_status_changed`

Thread status updated.

```json
{
  "event_type": "thread_status_changed",
  "thread_id": "thread-20260121-143052",
  "old_status": "active",
  "new_status": "resolved",
  "changed_by": "merlin",
  "reason": "Decision finalized and documented"
}
```

Status values: `active`, `paused`, `blocked`, `resolved`, `archived`

### `merlin_injected`

Merlin injected a message into a thread.

```json
{
  "event_type": "merlin_injected",
  "message_id": "msg-20260121-145000-merlin",
  "thread_id": "thread-20260121-143052",
  "content": "Hold on — we need to consider the extraction-team event schema too.",
  "action": "comment"
}
```

Action values: `comment`, `pause`, `redirect`, `approve`, `reject`

### `annotation_added`

Merlin added a note to a decision or message.

```json
{
  "event_type": "annotation_added",
  "annotation_id": "ann-20260121-150000",
  "target_type": "decision",
  "target_id": "dec-20260121-144000",
  "annotated_by": "merlin",
  "content": "This aligns with our extraction-team conventions. Good precedent."
}
```

### `mode_changed`

Observation mode changed.

```json
{
  "event_type": "mode_changed",
  "old_mode": "passive",
  "new_mode": "advisory",
  "changed_by": "merlin"
}
```

### `mediator_enriched`

Mediator added enrichment to a message.

```json
{
  "event_type": "mediator_enriched",
  "message_id": "msg-20260121-143052-x1y2z3",
  "enrichments": {
    "summary": "Aleph is asking about artifact versioning strategy...",
    "tags": ["architecture", "versioning", "s3"],
    "priority_suggestion": "high",
    "related_decisions": ["dec-20260115-001"]
  }
}
```

---

## Event ID Format

```
{type_prefix}-{date}-{time}-{random}

evt-20260121-143052-a1b2c3    # generic event
msg-20260121-143052-x1y2z3    # message
dec-20260121-144000           # decision
art-20260121-143100           # artifact
ann-20260121-150000           # annotation
thread-20260121-143052        # thread
```

---

## Idempotency

Events are idempotent by `event_id`. If an event with the same ID is written twice, the second write is ignored.

The indexer uses `event_id` to track what has been materialized to SurrealDB.

---

## Compaction (Future)

For long-running systems, events can be compacted:

1. Snapshot current SurrealDB state
2. Archive old events to `data/events-archive/`
3. Start fresh `events.jsonl` with a `snapshot_loaded` event

This is not needed for v0.1.
