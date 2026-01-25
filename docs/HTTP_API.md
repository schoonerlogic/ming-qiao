# Ming-Qiao HTTP API Specification

**Consumers:** Thales (Claude Chat), Merlin (Dashboard)  
**Framework:** Axum  
**Default Port:** 7777  
**Base URL:** `http://localhost:7777`

---

## Overview

The HTTP API serves two purposes:

1. **Thales Interface** — REST endpoints for reading/sending messages
2. **Merlin Dashboard** — Static UI + WebSocket for real-time updates

---

## Authentication

v0.1 is local-only with no authentication. The server binds to `127.0.0.1` only.

Future: Add bearer token or mTLS for remote access.

---

## REST Endpoints

### Inbox

#### `GET /api/inbox/{agent}`

Get pending messages for an agent.

**Path Parameters:**
- `agent` — Agent ID (`thales`, `aleph`, `merlin`)

**Query Parameters:**
- `unread_only` — boolean, default `true`
- `limit` — integer, default `20`
- `from` — filter by sender

**Response:**

```json
{
  "agent": "thales",
  "messages": [
    {
      "message_id": "msg-20260121-143052-x1y2z3",
      "thread_id": "thread-20260121-143052",
      "from_agent": "aleph",
      "subject": "Review S3 data strategy",
      "preview": "Beta's proposal looks good but I have questions...",
      "priority": "high",
      "sent_at": "2026-01-21T14:30:52Z",
      "read": false,
      "artifact_count": 1
    }
  ],
  "unread_count": 2,
  "total_count": 5
}
```

---

### Threads

#### `GET /api/threads`

List threads.

**Query Parameters:**
- `status` — `active`, `paused`, `blocked`, `resolved`, `archived`, `all`
- `participant` — filter by agent
- `limit` — default `20`
- `offset` — for pagination

**Response:**

```json
{
  "threads": [
    {
      "thread_id": "thread-20260121-143052",
      "subject": "Review S3 data strategy",
      "participants": ["aleph", "thales"],
      "status": "active",
      "started_at": "2026-01-21T14:30:52Z",
      "last_message_at": "2026-01-21T14:40:00Z",
      "message_count": 4,
      "decision_count": 1,
      "unread_count": 1
    }
  ],
  "total": 12
}
```

#### `GET /api/thread/{id}`

Get full thread with messages.

**Response:**

```json
{
  "thread_id": "thread-20260121-143052",
  "subject": "Review S3 data strategy",
  "participants": ["aleph", "thales"],
  "status": "active",
  "started_at": "2026-01-21T14:30:52Z",
  "messages": [
    {
      "message_id": "msg-20260121-143052-x1y2z3",
      "from_agent": "aleph",
      "to_agent": "thales",
      "content": "Beta's proposal looks good but I have questions about artifact versioning...",
      "priority": "high",
      "sent_at": "2026-01-21T14:30:52Z",
      "read_at": null,
      "artifact_refs": [
        {
          "artifact_id": "art-20260121-143100",
          "path": "artifacts/s3-strategy-v0.1.md",
          "sha256": "abc123..."
        }
      ]
    },
    {
      "message_id": "msg-20260121-143500-abc123",
      "from_agent": "thales",
      "to_agent": "aleph",
      "content": "The Council reviewed this. Key requirement: derived artifacts must carry extractor identity...",
      "priority": "normal",
      "sent_at": "2026-01-21T14:35:00Z",
      "read_at": "2026-01-21T14:36:00Z",
      "artifact_refs": []
    }
  ],
  "decisions": [
    {
      "decision_id": "dec-20260121-144000",
      "question": "How should we version derived artifacts?",
      "resolution": "Extractor version in path",
      "status": "approved"
    }
  ]
}
```

#### `POST /api/thread/{id}/reply`

Post a reply to a thread.

**Request Body:**

```json
{
  "from_agent": "thales",
  "content": "The Council reviewed this. Key requirement...",
  "priority": "normal",
  "artifact_refs": []
}
```

**Response:**

```json
{
  "message_id": "msg-20260121-143500-abc123",
  "thread_id": "thread-20260121-143052",
  "sent_at": "2026-01-21T14:35:00Z"
}
```

#### `POST /api/threads`

Create a new thread (Merlin or direct API use).

**Request Body:**

```json
{
  "subject": "Architecture review needed",
  "from_agent": "merlin",
  "to_agent": "thales",
  "content": "Please review the ming-qiao architecture...",
  "priority": "high"
}
```

#### `PATCH /api/thread/{id}`

Update thread status.

**Request Body:**

```json
{
  "status": "paused",
  "reason": "Waiting for extraction-team alignment"
}
```

---

### Messages

#### `GET /api/message/{id}`

Get single message.

**Response:**

```json
{
  "message_id": "msg-20260121-143052-x1y2z3",
  "thread_id": "thread-20260121-143052",
  "from_agent": "aleph",
  "to_agent": "thales",
  "subject": "Review S3 data strategy",
  "content": "Beta's proposal looks good but I have questions...",
  "priority": "high",
  "sent_at": "2026-01-21T14:30:52Z",
  "read_at": null,
  "artifact_refs": [...],
  "context_refs": [...]
}
```

#### `PATCH /api/message/{id}`

Update message (mark read).

**Request Body:**

```json
{
  "read": true
}
```

---

### Artifacts

#### `GET /api/artifacts`

List shared artifacts.

**Query Parameters:**
- `shared_by` — filter by agent
- `limit` — default `50`

**Response:**

```json
{
  "artifacts": [
    {
      "artifact_id": "art-20260121-143100",
      "path": "artifacts/s3-strategy-v0.1.md",
      "shared_by": "aleph",
      "sha256": "abc123...",
      "bytes": 4523,
      "content_type": "text/markdown",
      "description": "S3 data strategy proposal",
      "shared_at": "2026-01-21T14:31:00Z"
    }
  ]
}
```

#### `GET /api/artifacts/{path}`

Download artifact content.

**Response:** Raw file content with appropriate `Content-Type` header.

---

### Decisions

#### `GET /api/decisions`

Query decisions.

**Query Parameters:**
- `q` — search query
- `status` — `pending`, `approved`, `rejected`, `superseded`, `all`
- `thread_id` — filter by thread
- `limit` — default `20`

**Response:**

```json
{
  "decisions": [
    {
      "decision_id": "dec-20260121-144000",
      "thread_id": "thread-20260121-143052",
      "question": "How should we version derived artifacts?",
      "resolution": "Extractor version in path",
      "rationale": "Derived artifacts must carry extractor identity...",
      "decided_by": "thales",
      "approved_by": "merlin",
      "status": "approved",
      "decided_at": "2026-01-21T14:40:00Z"
    }
  ],
  "total": 5
}
```

#### `GET /api/decisions/{id}`

Get single decision with full detail.

#### `POST /api/decisions/{id}/approve`

Approve a pending decision (Merlin only).

**Request Body:**

```json
{
  "comment": "Good call. Proceed."
}
```

#### `POST /api/decisions/{id}/reject`

Reject a pending decision (Merlin only).

**Request Body:**

```json
{
  "reason": "Need to consider backward compatibility first"
}
```

---

### Merlin Actions

#### `POST /api/inject`

Inject a message into a thread.

**Request Body:**

```json
{
  "thread_id": "thread-20260121-143052",
  "content": "Hold on — we need to consider the extraction-team event schema too.",
  "action": "comment"
}
```

Actions: `comment`, `pause`, `redirect`, `approve`, `reject`

#### `POST /api/annotate`

Add annotation to a message or decision.

**Request Body:**

```json
{
  "target_type": "decision",
  "target_id": "dec-20260121-144000",
  "content": "This aligns with our extraction-team conventions."
}
```

#### `GET /api/config`

Get current configuration.

**Response:**

```json
{
  "mode": "advisory",
  "notify_on": {
    "priority": ["high", "critical"],
    "keywords": ["breaking change", "security"],
    "decision_type": ["architectural"]
  }
}
```

#### `PATCH /api/config`

Update configuration.

**Request Body:**

```json
{
  "mode": "gated"
}
```

---

### Search

#### `GET /api/search`

Full-text search across messages and decisions.

**Query Parameters:**
- `q` — search query (required)
- `type` — `messages`, `decisions`, `all`
- `limit` — default `20`

**Response:**

```json
{
  "query": "artifact versioning",
  "results": [
    {
      "type": "decision",
      "id": "dec-20260121-144000",
      "snippet": "...How should we version derived artifacts?...",
      "score": 0.92
    },
    {
      "type": "message",
      "id": "msg-20260121-143052-x1y2z3",
      "snippet": "...questions about artifact versioning...",
      "score": 0.78
    }
  ],
  "total": 2
}
```

---

## WebSocket

### `GET /ws`

WebSocket connection for real-time updates.

**Connection:** Upgrade to WebSocket at `/ws`

### Server → Client Messages

```typescript
// New message in any thread
{ "type": "message", "thread_id": "...", "message": {...} }

// Decision pending approval
{ "type": "decision_pending", "decision": {...} }

// Thread status changed
{ "type": "thread_status", "thread_id": "...", "status": "resolved" }

// Agent typing indicator
{ "type": "agent_typing", "agent": "aleph", "thread_id": "..." }

// Mode changed
{ "type": "mode_changed", "old_mode": "passive", "new_mode": "gated" }

// Connection established
{ "type": "connected", "mode": "advisory", "unread_count": 3 }
```

### Client → Server Messages

```typescript
// Inject message
{ "type": "inject", "thread_id": "...", "content": "...", "action": "comment" }

// Approve decision
{ "type": "approve", "decision_id": "..." }

// Reject decision
{ "type": "reject", "decision_id": "...", "reason": "..." }

// Change mode
{ "type": "set_mode", "mode": "gated" }

// Subscribe to specific thread
{ "type": "subscribe", "thread_id": "..." }

// Mark message read
{ "type": "mark_read", "message_id": "..." }
```

---

## Static Files

### `GET /ui`

Serves the Svelte dashboard SPA.

### `GET /ui/{path}`

Serves static assets (JS, CSS, images).

---

## Error Responses

All errors follow this format:

```json
{
  "error": {
    "code": "NOT_FOUND",
    "message": "Thread thread-20260121-999999 not found"
  }
}
```

HTTP Status Codes:

| Code | Meaning |
|------|---------|
| 200 | Success |
| 201 | Created |
| 400 | Bad request / invalid input |
| 404 | Resource not found |
| 409 | Conflict (e.g., decision already approved) |
| 500 | Internal server error |

---

## CORS

For development, CORS is enabled for `localhost:*`. Production should disable or restrict.
