# TASK: Agent Notification System — Autonomous Message Delivery

**Assigned to:** Aleph
**Priority:** High
**Thread:** (to be created)
**Date:** 2026-02-23

## Problem

When Thales sends a message to Aleph (or any agent sends to any other), the recipient
does not see it until a human says "check your inbox" or the agent happens to check
on session start. Merlin must act as message relay, which defeats the purpose of
ming-qiao and prevents the captain from going below deck to study.

## Goal

Any message sent through ming-qiao should reach its recipient agent within seconds,
without human intervention. The captain should be able to trust the crew to communicate
autonomously while he remains deep in research.

## Design

### Architecture: Watcher-per-Agent with Notification Files

Each agent that runs in an environment with file access gets a **notification watcher**
that writes to a well-known file. The agent's host environment (Claude Code, Goose,
Gemini, etc.) monitors this file as part of its operating protocol.

### Implementation Plan

#### Step 1: Add per-agent notification watchers to ming-qiao.toml

For each agent, add a watcher that filters for messages addressed TO that agent:

```toml
# Aleph notification — triggers when a message is sent TO aleph
[[watchers]]
agent = "aleph-notify"
role = "observer"
subjects = ["am.events.mingqiao"]

[watchers.filter]
event_types = ["message_sent"]

[watchers.action]
type = "file_append"
path = "/Users/proteus/astralmaris/ming-qiao/notifications/aleph.jsonl"

# Thales notification
[[watchers]]
agent = "thales-notify"
role = "observer"
subjects = ["am.events.mingqiao"]

[watchers.filter]
event_types = ["message_sent"]

[watchers.action]
type = "file_append"
path = "/Users/proteus/astralmaris/ming-qiao/notifications/thales.jsonl"

# Luban notification
[[watchers]]
agent = "luban-notify"
role = "observer"
subjects = ["am.events.mingqiao"]

[watchers.filter]
event_types = ["message_sent"]

[watchers.action]
type = "file_append"
path = "/Users/proteus/astralmaris/ming-qiao/notifications/luban.jsonl"

# Laozi-Jung keeps existing stream watcher, plus gets notification
[[watchers]]
agent = "laozi-jung-notify"
role = "observer"
subjects = ["am.events.mingqiao"]

[watchers.filter]
event_types = ["message_sent"]

[watchers.action]
type = "file_append"
path = "/Users/proteus/astralmaris/ming-qiao/notifications/laozi-jung.jsonl"
```

**Problem with current approach:** The watcher dispatch doesn't filter by message
recipient — it fires on ALL message_sent events for ALL watchers. We need recipient
filtering.

#### Step 2: Add recipient filter to WatcherFilter

Extend `WatcherFilter` in `src/watcher/config.rs`:

```rust
pub struct WatcherFilter {
    /// Event types to include. Empty means all event types pass.
    #[serde(default)]
    pub event_types: Vec<String>,
    
    /// Only match messages addressed TO these agents.
    /// Empty means all recipients pass.
    /// Supports: specific agent ID, "council", "all"
    #[serde(default)]
    pub recipients: Vec<String>,
}
```

And update the dispatch loop in `dispatch.rs` to check the recipient field:

```rust
// After event_type filter check, add recipient filter:
if !watcher.recipients.is_empty() {
    if let EventPayload::Message(m) = &event.payload {
        let matches_recipient = watcher.recipients.iter().any(|r| {
            r == &m.to || r == "council" && m.to == "council" || r == "all" && m.to == "all"
        });
        if !matches_recipient {
            continue;
        }
    }
}
```

TOML then becomes:

```toml
[[watchers]]
agent = "aleph-notify"
role = "observer"
subjects = ["am.events.mingqiao"]

[watchers.filter]
event_types = ["message_sent"]
recipients = ["aleph", "council", "all"]

[watchers.action]
type = "file_append"
path = "/Users/proteus/astralmaris/ming-qiao/notifications/aleph.jsonl"
```

Each agent's watcher fires only on messages addressed to them, to "council", or to "all".

#### Step 3: Notification file format

Each line in the notification file should be a compact, actionable alert:

```json
{"ts":"2026-02-23T14:08:28Z","from":"thales","subject":"Infrastructure reply","intent":"discuss","thread":"019c8ad2-8f8e-...","event_id":"019c8ad9-c139-..."}
```

This means creating a new compact format (like EventLine but for notifications specifically),
or reusing EventLine which already has from/to/subject/content_preview.

EventLine already works well for this. The existing file_append action writes exactly
this format. No new serialization needed.

#### Step 4: Agent protocol update

Each agent's CLAUDE.md (or equivalent operational prompt) should include:

```
## Notification Protocol

A notification file exists at:
  /Users/proteus/astralmaris/ming-qiao/notifications/{your-agent-id}.jsonl

When starting a session:
1. Check this file for new notifications since your last session
2. Read your full inbox via check_messages MCP tool
3. Respond to any messages with intent=request first

During a session:
- If you have file watching capability, monitor the notification file
- New lines mean new messages have arrived for you
- Read them and respond without waiting for human relay
```

#### Step 5: macOS notification (optional enhancement)

For agents that don't have file watching, add a third watcher action type:

```rust
pub enum WatcherAction {
    FileAppend { path: String },
    Webhook { url: String },
    SystemNotify { title: String },  // NEW: macOS notification via osascript
}
```

The SystemNotify action runs:
```bash
osascript -e 'display notification "Message from {from}: {subject}" with title "{title}"'
```

This gives Merlin (the human) a macOS notification when any agent sends a message,
even if no agent session is active. The human can then decide whether to check in
or let the agents handle it.

## Implementation Order

1. **Recipient filter on WatcherFilter** — extend the struct and dispatch logic (~30 min)
2. **Notification watchers in ming-qiao.toml** — one per agent (~10 min)
3. **Create notifications/ directory** — mkdir + .gitignore (~5 min)
4. **Test end-to-end** — send message, verify only recipient's file gets a line (~15 min)
5. **Update agent operational prompts** — CLAUDE.md and equivalents (~15 min)
6. **(Optional) SystemNotify action** — macOS osascript integration (~30 min)

## Success Criteria

- Thales sends a message to Aleph → only aleph.jsonl gets a new line
- Aleph sends to council → all agent notification files get a new line
- Agents can read their notification file at session start and know what's waiting
- No human relay required for inter-agent message awareness

## Notes

- The notification files should be .gitignored — they're ephemeral operational state
- Consider a `--clear-notifications` flag or MCP tool to truncate after reading
- The SystemNotify action for Merlin is low priority but high value — it means the
  captain gets a tap on the shoulder when the crew needs attention, even while studying
