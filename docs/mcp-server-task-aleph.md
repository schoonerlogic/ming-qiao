# Ming-Qiao MCP Server — Thales On-Ramp

**Date:** 2026-02-21
**Issued by:** Thales (Architect) on behalf of Proteus
**Assigned to:** Aleph
**Branch from:** develop (post golden-thread merge)

---

## Context

The golden thread proved the persistence path works:

```
POST /api/threads           → create thread + message → SurrealDB + Indexer
GET  /api/inbox/{agent}     → read messages from indexer
POST /api/thread/:id/reply  → reply within thread
```

Thales (Claude Desktop) cannot use HTTP endpoints directly. Thales interacts through MCP tools. This task wraps the proven HTTP operations as MCP tools so Thales can join the agent network.

**This is not new functionality.** This is exposing existing, tested operations through the MCP protocol. The HTTP handlers remain the source of truth — MCP tools call into the same persistence and indexer layer.

---

## Objective

Build an MCP server that exposes ming-qiao's thread operations as tools, connectable from Claude Desktop.

---

## Tools to Implement

### 1. `create_thread`

Create a new conversation thread between agents.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| from_agent | string | yes | Sending agent identifier |
| to_agent | string | yes | Receiving agent identifier |
| subject | string | yes | Thread subject (use `am.agent.*` convention) |
| content | string | yes | Message body |

**Returns:**

```json
{
  "thread_id": "uuid",
  "message_id": "uuid",
  "created_at": "ISO-8601"
}
```

**Behavior:** Same as `POST /api/threads`. Persists to SurrealDB, updates indexer, broadcasts to WebSocket.

---

### 2. `read_inbox`

Read pending messages for an agent.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| agent | string | yes | Agent whose inbox to read |

**Returns:**

```json
{
  "messages": [
    {
      "message_id": "uuid",
      "thread_id": "uuid",
      "from": "agent-name",
      "subject": "am.agent.council.*",
      "content": "message body",
      "created_at": "ISO-8601"
    }
  ]
}
```

**Behavior:** Same as `GET /api/inbox/{agent}`. Reads from indexer.

---

### 3. `reply_to_thread`

Reply within an existing thread.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| thread_id | string | yes | Thread to reply in |
| from_agent | string | yes | Sending agent identifier |
| content | string | yes | Reply body |

**Returns:**

```json
{
  "message_id": "uuid",
  "thread_id": "uuid",
  "created_at": "ISO-8601"
}
```

**Behavior:** Same as `POST /api/thread/:id/reply`. Looks up thread for recipient and subject, persists, indexes, broadcasts.

---

### 4. `list_threads`

List threads an agent is participating in.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| agent | string | yes | Agent whose threads to list |

**Returns:**

```json
{
  "threads": [
    {
      "thread_id": "uuid",
      "subject": "am.agent.council.*",
      "participants": ["aleph", "luban"],
      "message_count": 4,
      "last_message_at": "ISO-8601"
    }
  ]
}
```

**Behavior:** Maps to existing indexer queries. If no HTTP endpoint exists for this yet, read from the indexer directly — same pattern as the inbox handler.

---

### 5. `read_thread`

Read all messages in a specific thread.

**Parameters:**

| Name | Type | Required | Description |
|------|------|----------|-------------|
| thread_id | string | yes | Thread to read |

**Returns:**

```json
{
  "thread_id": "uuid",
  "subject": "am.agent.council.*",
  "messages": [
    {
      "message_id": "uuid",
      "from": "agent-name",
      "content": "message body",
      "created_at": "ISO-8601"
    }
  ]
}
```

**Behavior:** Maps to existing thread retrieval. If `GET /api/thread/:id` exists, mirror it. If not, query the indexer directly.

---

## Architecture

```
Claude Desktop (Thales)
    ↓ MCP protocol (stdio or SSE)
MCP Server
    ↓ direct function calls (not HTTP)
Persistence layer (SurrealDB) + Indexer
```

**Important:** The MCP server should call into the same Rust functions that the HTTP handlers use — not make HTTP requests to itself. Share the application state (`AppState`, persistence, indexer) between the HTTP server and MCP server.

If they run as the same binary (recommended), both the HTTP listener and MCP transport share the same `AppState`. If separate binaries, the MCP server needs its own SurrealDB and indexer connections.

---

## MCP Transport

Claude Desktop connects via **stdio** (standard input/output). The MCP server should support stdio transport at minimum.

```bash
# Claude Desktop config (~/.claude/claude_desktop_config.json or similar)
{
  "mcpServers": {
    "ming-qiao": {
      "command": "/path/to/ming-qiao",
      "args": ["mcp"],
      "env": {
        "SURREALDB_URL": "http://localhost:8000",
        "SURREALDB_USER": "root",
        "SURREALDB_PASS": "root"
      }
    }
  }
}
```

Consider: `cargo run mcp` for MCP mode vs `cargo run serve` for HTTP mode. Same binary, different entry points.

---

## Implementation Notes

- **v0.1 had 8 MCP tools already.** Check `src/mcp/` for existing MCP infrastructure — patterns, transport setup, tool registration. Build on what's there, don't rewrite.
- **Serde aliases are already in place** for `from`/`from_agent` and `to`/`to_agent`. MCP tool parameters should use the clearer `from_agent`/`to_agent` naming.
- **Error handling:** Return clear error messages. "Thread not found" with the thread_id, not generic failures. Thales will be reading these errors to decide what to do next.
- **Tool descriptions matter.** Claude Desktop uses them to decide when to invoke tools. Write descriptions that make the tool's purpose and constraints clear.

---

## Testing

### Unit tests

- Each tool handler called directly with mock state
- Verify correct persistence and indexer updates
- Verify error cases (missing thread, invalid agent name)

### Integration test: Three-agent golden thread

Once deployed and connected to Claude Desktop, we run:

```
Thales  → create_thread(to: "aleph", subject: "am.agent.council.three-way")
Aleph   → GET /api/inbox/aleph → sees Thales' message
Aleph   → POST /api/thread/:id/reply
Thales  → read_inbox(agent: "thales") → sees Aleph's reply
Thales  → create_thread(to: "luban", subject: "am.agent.council.three-way")
Luban   → GET /api/inbox/luban → sees Thales' message
```

This proves the MCP ↔ HTTP ↔ persistence path is unified.

---

## Deliverables

1. MCP server with 5 tools (`create_thread`, `read_inbox`, `reply_to_thread`, `list_threads`, `read_thread`)
2. stdio transport for Claude Desktop connection
3. Shared `AppState` with HTTP server (same binary, `cargo run mcp` entry point)
4. Unit tests for each tool handler
5. Claude Desktop configuration example

---

## Constraints

- Do NOT change existing HTTP handlers or their behavior
- Do NOT add new dependencies without flagging to Proteus
- Build on existing MCP infrastructure in `src/mcp/`
- All existing tests must continue to pass

---

## Completion Report

```
MCP SERVER — ALEPH REPORT

Tools implemented:
  - create_thread: [status]
  - read_inbox: [status]
  - reply_to_thread: [status]
  - list_threads: [status]
  - read_thread: [status]

Transport: [stdio / SSE / both]
Entry point: [cargo run mcp / other]

New tests: [count]
Existing tests: [pass count / total]

Claude Desktop config: [path or example]

Dependencies added: [list, or "none"]

Ready for three-agent golden thread: [yes/no]
```

---

## After Success

1. **Proteus** configures Claude Desktop to connect to ming-qiao MCP
2. **Three-agent golden thread** — Thales, Aleph, Luban prove unified messaging
3. **Aleph** wires NATS publish-on-write for real-time notification
4. **Luban** receives bounded tasks via the now-live network

The architect joins the bridge. 明桥迎接智者。

---

*Issued by Thales. Proteus coordinates execution.*
