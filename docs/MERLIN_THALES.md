# Merlin and Thales Communication Guide

**Version:** 0.1
**Last Updated:** 2026-01-27

---

## Overview

Ming-Qiao provides two communication channels for non-Aleph agents:

1. **Merlin (Proteus)** — Human operator with real-time notifications and intervention capabilities
2. **Thales** — Architect agent via HTTP API and WebSocket event stream

---

## Merlin Communication

Merlin (the human operator, Proteus) has two primary interfaces:

### 1. Real-time Notifications (WebSocket)

**Endpoint:** `ws://localhost:7777/merlin/notifications`

Connects Merlin to a real-time stream of notifications based on observation mode.

**Connection:**

```javascript
const ws = new WebSocket('ws://localhost:7777/merlin/notifications');

ws.onmessage = (event) => {
  const notification = JSON.parse(event.data);

  switch (notification.type) {
    case 'connected':
      console.log('Connected to ming-qiao');
      console.log('Current mode:', notification.mode);
      break;

    case 'priorityAlert':
      // High-priority message requires attention
      console.log('Priority alert:', notification.reason);
      displayEvent(notification.event);
      break;

    case 'keywordDetected':
      // Keyword found in message
      console.log('Keyword detected:', notification.keyword);
      displayEvent(notification.event);
      break;

    case 'decisionReview':
      // Decision requires review
      console.log('Decision review needed:', notification.decisionType);
      displayEvent(notification.event);
      break;

    case 'actionBlocked':
      // Action blocked in gated mode
      console.log('Action blocked:', notification.reason);
      displayEvent(notification.event);
      break;
  }
};
```

**Notification Types:**

| Type | Trigger | Data |
|------|---------|------|
| `PriorityAlert` | Message with high/critical priority | `event`, `reason` |
| `KeywordDetected` | Message contains trigger keyword | `event`, `keyword` |
| `DecisionReview` | Decision matches advisory type | `event`, `decisionType` |
| `ActionBlocked` | Gated mode blocked action | `event`, `reason` |
| `StatusUpdate` | System status change | `message`, `timestamp` |

### 2. Interventions (WebSocket → Server)

Merlin can send intervention messages through the same WebSocket connection:

**Inject a message:**

```json
{
  "action": "injectMessage",
  "threadId": "thread-uuid",
  "from": "merlin",
  "content": "Please pause this discussion and reconsider the approach."
}
```

**Approve a decision:**

```json
{
  "action": "approveDecision",
  "decisionId": "decision-uuid",
  "reason": "Approved - this aligns with our architecture goals."
}
```

**Reject a decision:**

```json
{
  "action": "rejectDecision",
  "decisionId": "decision-uuid",
  "reason": "Too risky - we need more research first."
}
```

**Change observation mode:**

```json
{
  "action": "setMode",
  "mode": "advisory"  // or "passive" or "gated"
}
```

### 3. HTTP API (Dashboard)

Thales' HTTP API is also available to Merlin:

- `GET /api/threads` — List all threads
- `GET /api/thread/{id}` — Get thread with messages
- `POST /api/thread/{id}/reply` — Reply to thread
- `GET /api/decisions` — List decisions
- `POST /api/inject` — Inject Merlin message
- `PATCH /api/config` — Update configuration (mode, triggers)

---

## Thales Communication

Thales (the architect agent) communicates via HTTP API and WebSocket event stream.

### 1. Event Stream (WebSocket)

**Endpoint:** `ws://localhost:7777/ws`

Real-time stream of all events written to the log.

**Connection:**

```javascript
const ws = new WebSocket('ws://localhost:7777/ws');

ws.onmessage = (event) => {
  const data = JSON.parse(event.data);

  if (data.type === 'event') {
    const event = data.event;
    console.log('New event:', event.eventType, event.agentId);

    // Process event based on type
    switch (event.eventType) {
      case 'MessageSent':
        // Handle new message
        break;
      case 'DecisionRecorded':
        // Handle decision
        break;
      // ... other event types
    }
  }
};
```

**Query Parameters:**

- `?agent=thales` — Filter events for specific agent
- `?eventTypes=message_sent,decision_recorded` — Filter by event types (comma-separated)

### 2. REST API

Thales can use these endpoints to read/write:

**Read Inbox:**

```bash
curl http://localhost:7777/api/inbox/thales
```

**Get Thread:**

```bash
curl http://localhost:7777/api/thread/{thread-id}
```

**Reply to Thread:**

```bash
curl -X POST http://localhost:7777/api/thread/{thread-id}/reply \
  -H "Content-Type: application/json" \
  -d '{
    "fromAgent": "thales",
    "content": "Here is my analysis...",
    "priority": "normal"
  }'
```

**List Decisions:**

```bash
curl http://localhost:7777/api/decisions
```

**Get Decision:**

```bash
curl http://localhost:7777/api/decisions/{decision-id}
```

---

## Observation Modes

Ming-Qiao supports three observation modes (set via `ming-qiao.toml` or API):

### Passive Mode (Default)

- All messages flow freely
- No notifications sent to Merlin
- Events logged for review
- Merlin can still read history via HTTP API

```toml
[observation]
mode = "passive"
```

### Advisory Mode

- Merlin notified on triggers
- No blocking
- Notifications for:
  - High/critical priority messages
  - Keywords in messages
  - Certain decision types

```toml
[observation]
mode = "advisory"

[observation.notify_on]
priority = ["high", "critical"]
keywords = ["breaking change", "security", "blocked"]
decision_type = ["architectural"]
```

### Gated Mode

- All events notified to Merlin
- Certain actions require approval
- Decisions can be blocked until Merlin approves

```toml
[observation]
mode = "gated"

[observation.gate]
decision_type = ["architectural", "external"]
```

---

## Configuration File

`ming-qiao.toml` (in project root):

```toml
[observation]
mode = "advisory"  # passive | advisory | gated

[observation.notify_on]
priority = ["high", "critical"]
keywords = [
  "breaking change",
  "security",
  "blocked",
  "deadline",
  "cost"
]
decision_type = ["architectural", "external"]

[observation.gate]
decision_type = ["architectural", "external"]

[data_dir]
path = "data"

[server]
port = 7777
```

Update configuration at runtime:

```bash
# Change mode to advisory
curl -X PATCH http://localhost:7777/api/config \
  -H "Content-Type: application/json" \
  -d '{"mode": "advisory"}'

# Add keyword trigger
curl -X PATCH http://localhost:7777/api/config \
  -H "Content-Type: application/json" \
  -d '{
    "notify_on": {
      "keywords": ["urgent", "emergency"]
    }
  }'
```

---

## Testing

### Test Merlin Connection

```bash
# Start server
./target/debug/ming-qiao serve

# In another terminal, test WebSocket with websocat
websocat ws://localhost:7777/merlin/notifications

# Or test HTTP upgrade
curl -i -N \
  -H "Connection: Upgrade" \
  -H "Upgrade: websocket" \
  -H "Sec-WebSocket-Version: 13" \
  -H "Sec-WebSocket-Key: test" \
  http://localhost:7777/merlin/notifications
```

### Test Thales Connection

```bash
# Test event stream
curl -i -N \
  -H "Connection: Upgrade" \
  -H "Upgrade: websocket" \
  -H "Sec-WebSocket-Version: 13" \
  -H "Sec-WebSocket-Key: test" \
  http://localhost:7777/ws

# Read inbox
curl http://localhost:7777/api/inbox/thales | jq .

# List threads
curl http://localhost:7777/api/threads | jq .
```

### Test Notification Flow

1. Start server
2. Connect to `ws://localhost:7777/merlin/notifications`
3. Set mode to advisory via API or config
4. Send a high-priority message via MCP tool
5. Verify notification received

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Merlin (Proteus)                       │
│  ┌──────────────────┐         ┌──────────────────┐         │
│  │  WebSocket:      │         │  HTTP API:       │         │
│  │  Notifications   │         │  Dashboard       │         │
│  │  Interventions   │         │  Config/Control  │         │
│  └──────────────────┘         └──────────────────┘         │
└─────────────┬───────────────────────────┬──────────────────┘
              │                           │
              ▼                           ▼
┌─────────────────────────────────────────────────────────────┐
│                         ming-qiao                          │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐  │
│  │  /merlin/    │    │    /ws       │    │   /api/*     │  │
│  │notifications │    │  (events)    │    │   (REST)     │  │
│  └──────────────┘    └──────────────┘    └──────────────┘  │
│                                                               │
│  ┌────────────────────────────────────────────────────────┐ │
│  │  Event Log → Indexer → Observation Mode → Notifier      │ │
│  └────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
              │                           │
              ▼                           ▼
┌─────────────────────┐         ┌─────────────────────┐
│      Aleph          │         │      Thales         │
│   (MCP tools)       │         │   (HTTP/WebSocket)  │
└─────────────────────┘         └─────────────────────┘
```

---

## Next Steps

1. **UI Integration** — Connect Svelte UI to Merlin notification stream
2. **Thales Client** — Build Thales client library for agent-to-agent messaging
3. **Intervention Processing** — Implement Merlin intervention handlers (inject, approve, reject)
4. **Decision Workflow** — Full gated mode with pending/approved/rejected states

---

## References

- **Event Schema:** `docs/EVENTS.md`
- **HTTP API:** `docs/HTTP_API.md`
- **Architecture:** `docs/ARCHITECTURE.md`
- **Configuration:** `ming-qiao.toml`
