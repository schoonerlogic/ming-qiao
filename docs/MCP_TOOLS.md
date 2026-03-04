# Ming-Qiao MCP Tools Specification

**Consumer:** Aleph (Claude CLI)  
**Transport:** stdio (JSON-RPC over stdin/stdout)  
**Protocol:** Model Context Protocol (MCP)

---

## Overview

Aleph connects to Ming-Qiao via MCP, calling tools to send/receive messages, share artifacts, and query decisions. The MCP server runs as a subprocess managed by Claude CLI.

---

## Configuration

### Claude CLI Config (`~/.config/claude/mcp.json`)

```json
{
  "mcpServers": {
    "ming-qiao": {
      "command": "ming-qiao",
      "args": ["mcp-serve"],
      "env": {
        "MING_QIAO_DATA_DIR": "/path/to/ming-qiao/data",
        "MING_QIAO_AGENT_ID": "aleph"
      }
    }
  }
}
```

### Environment Variables

| Variable | Required | Description |
|----------|----------|-------------|
| `MING_QIAO_DATA_DIR` | Yes | Path to data directory |
| `MING_QIAO_AGENT_ID` | Yes | Agent identity (`aleph`) |
| `MING_QIAO_CONFIG` | No | Path to `ming-qiao.toml` |

---

## Tools

### `send_message`

Send a message to another agent.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "to": {
      "type": "string",
      "description": "Recipient agent ID (e.g., 'thales', 'merlin')"
    },
    "subject": {
      "type": "string",
      "description": "Message subject line"
    },
    "content": {
      "type": "string",
      "description": "Message body (markdown supported)"
    },
    "thread_id": {
      "type": "string",
      "description": "Existing thread ID to reply to (optional, creates new thread if omitted)"
    },
    "priority": {
      "type": "string",
      "enum": ["low", "normal", "high", "critical"],
      "default": "normal",
      "description": "Message priority"
    },
    "artifact_refs": {
      "type": "array",
      "items": { "type": "string" },
      "description": "Paths to artifacts to attach (must be shared first)"
    },
    "context_refs": {
      "type": "array",
      "items": { "type": "string" },
      "description": "IDs of related decisions or threads"
    }
  },
  "required": ["to", "subject", "content"]
}
```

**Example Call:**

```json
{
  "name": "send_message",
  "arguments": {
    "to": "thales",
    "subject": "Review S3 data strategy",
    "content": "Beta's proposal looks good but I have questions about artifact versioning...",
    "priority": "high",
    "artifact_refs": ["artifacts/s3-strategy-v0.1.md"]
  }
}
```

**Response:**

```json
{
  "type": "text",
  "text": "Message sent: msg-20260121-143052-x1y2z3\nThread: thread-20260121-143052"
}
```

---

### `check_messages`

Check inbox for new messages.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "unread_only": {
      "type": "boolean",
      "default": true,
      "description": "Only return unread messages"
    },
    "from_agent": {
      "type": "string",
      "description": "Filter by sender agent ID"
    },
    "limit": {
      "type": "integer",
      "default": 10,
      "description": "Maximum messages to return"
    }
  }
}
```

**Example Call:**

```json
{
  "name": "check_messages",
  "arguments": {
    "unread_only": true
  }
}
```

**Response:**

```json
{
  "type": "text",
  "text": "## Inbox (2 unread)\n\n🔵 **Re: S3 data strategy** from thales (14:35)\n   Thread: thread-20260121-143052\n   ID: msg-20260121-143500-abc123\n\n🔵 **Prescreen design feedback** from merlin (14:20)\n   Thread: thread-20260121-142000\n   ID: msg-20260121-142000-def456"
}
```

---

### `read_message`

Read full content of a specific message.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "message_id": {
      "type": "string",
      "description": "Message ID to read"
    },
    "mark_read": {
      "type": "boolean",
      "default": true,
      "description": "Mark message as read"
    }
  },
  "required": ["message_id"]
}
```

**Example Call:**

```json
{
  "name": "read_message",
  "arguments": {
    "message_id": "msg-20260121-143500-abc123"
  }
}
```

**Response:**

```json
{
  "type": "text",
  "text": "## Message: msg-20260121-143500-abc123\n\n**From:** thales\n**To:** aleph\n**Subject:** Re: S3 data strategy\n**Thread:** thread-20260121-143052\n**Time:** 2026-01-21T14:35:00Z\n**Priority:** high\n\n---\n\nThe Council reviewed this. Key requirement: derived artifacts must carry extractor identity and content hash...\n\n### Attachments\n- None\n\n### Related\n- Decision: dec-20260115-001"
}
```

---

### `request_review`

Ask Thales to review an artifact (convenience wrapper around send_message).

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "artifact_path": {
      "type": "string",
      "description": "Path to artifact to review (will be shared automatically)"
    },
    "question": {
      "type": "string",
      "description": "Specific question or focus area for review"
    },
    "context": {
      "type": "string",
      "description": "Additional context for the reviewer"
    },
    "priority": {
      "type": "string",
      "enum": ["low", "normal", "high", "critical"],
      "default": "normal"
    }
  },
  "required": ["artifact_path", "question"]
}
```

**Example Call:**

```json
{
  "name": "request_review",
  "arguments": {
    "artifact_path": "/Users/proteus/astralmaris/extraction-team/docs/S3_STRATEGY.md",
    "question": "Is the artifact versioning approach compatible with our event schema?",
    "priority": "high"
  }
}
```

**Response:**

```json
{
  "type": "text",
  "text": "Review requested:\n- Artifact shared: artifacts/S3_STRATEGY.md (sha256: abc123...)\n- Message sent: msg-20260121-143052-x1y2z3\n- Thread: thread-20260121-143052"
}
```

---

### `share_artifact`

Share a file for other agents to access.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "source_path": {
      "type": "string",
      "description": "Local path to file to share"
    },
    "description": {
      "type": "string",
      "description": "Brief description of the artifact"
    },
    "target_name": {
      "type": "string",
      "description": "Name in artifacts directory (optional, defaults to filename)"
    }
  },
  "required": ["source_path"]
}
```

**Example Call:**

```json
{
  "name": "share_artifact",
  "arguments": {
    "source_path": "/Users/proteus/astralmaris/extraction-team/docs/EVENTS.md",
    "description": "Extraction team event log schema"
  }
}
```

**Response:**

```json
{
  "type": "text",
  "text": "Artifact shared:\n- Path: artifacts/EVENTS.md\n- SHA256: def456...\n- Bytes: 3421\n- ID: art-20260121-143100"
}
```

---

### `get_decision`

Retrieve a past decision by ID or query.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "decision_id": {
      "type": "string",
      "description": "Specific decision ID"
    },
    "query": {
      "type": "string",
      "description": "Search query (if no decision_id)"
    },
    "limit": {
      "type": "integer",
      "default": 5,
      "description": "Max results for query"
    }
  }
}
```

**Example Call (by ID):**

```json
{
  "name": "get_decision",
  "arguments": {
    "decision_id": "dec-20260121-144000"
  }
}
```

**Response:**

```json
{
  "type": "text",
  "text": "## Decision: dec-20260121-144000\n\n**Question:** How should we version derived artifacts?\n\n**Resolution:** Extractor version in path\n\n**Rationale:** Derived artifacts must carry extractor identity and content hash for reproducibility...\n\n**Decided by:** thales\n**Status:** approved\n**Thread:** thread-20260121-143052\n**Date:** 2026-01-21T14:40:00Z"
}
```

**Example Call (by query):**

```json
{
  "name": "get_decision",
  "arguments": {
    "query": "artifact versioning"
  }
}
```

---

### `list_threads`

List active and recent threads.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "status": {
      "type": "string",
      "enum": ["active", "paused", "blocked", "resolved", "archived", "all"],
      "default": "active"
    },
    "limit": {
      "type": "integer",
      "default": 10
    },
    "participant": {
      "type": "string",
      "description": "Filter by participant agent"
    }
  }
}
```

**Example Call:**

```json
{
  "name": "list_threads",
  "arguments": {
    "status": "active"
  }
}
```

**Response:**

```json
{
  "type": "text",
  "text": "## Active Threads\n\n1. **S3 data strategy** (thread-20260121-143052)\n   Participants: aleph, thales\n   Last: 14:40 | Messages: 4 | Decisions: 1\n\n2. **Prescreen design** (thread-20260121-142000)\n   Participants: aleph, merlin\n   Last: 14:20 | Messages: 2 | Decisions: 0"
}
```

---

### `record_decision`

Record a decision from the current conversation.

**Input Schema:**

```json
{
  "type": "object",
  "properties": {
    "thread_id": {
      "type": "string",
      "description": "Thread where decision was made"
    },
    "question": {
      "type": "string",
      "description": "What was being decided"
    },
    "resolution": {
      "type": "string",
      "description": "What was decided"
    },
    "rationale": {
      "type": "string",
      "description": "Why this option was chosen"
    },
    "options_considered": {
      "type": "array",
      "items": { "type": "string" },
      "description": "Alternatives that were evaluated"
    }
  },
  "required": ["thread_id", "question", "resolution", "rationale"]
}
```

**Example Call:**

```json
{
  "name": "record_decision",
  "arguments": {
    "thread_id": "thread-20260121-143052",
    "question": "How should we version derived artifacts?",
    "resolution": "Include extractor name and version in the path",
    "rationale": "This makes dataset versions reproducible and allows tracking which extractor produced which output",
    "options_considered": [
      "Content-addressed paths only",
      "Extractor version in path",
      "Separate manifest tracking"
    ]
  }
}
```

**Response:**

```json
{
  "type": "text",
  "text": "Decision recorded: dec-20260121-144000\nStatus: pending (awaiting Merlin approval in gated mode)"
}
```

---

## Error Handling

Tools return errors in the standard MCP format:

```json
{
  "type": "error",
  "error": {
    "code": "NOT_FOUND",
    "message": "Message msg-20260121-999999 not found"
  }
}
```

Error codes:

| Code | Description |
|------|-------------|
| `NOT_FOUND` | Resource does not exist |
| `INVALID_INPUT` | Invalid arguments |
| `PERMISSION_DENIED` | Agent cannot perform this action |
| `CONFLICT` | Resource already exists or state conflict |
| `INTERNAL_ERROR` | Unexpected server error |

---

## Agent Identity

The MCP server identifies the calling agent via `MING_QIAO_AGENT_ID`. This is used for:

- `from_agent` field in sent messages
- Inbox filtering (only see messages to self)
- Decision attribution

Agents cannot impersonate other agents.
