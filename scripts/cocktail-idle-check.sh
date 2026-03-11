#!/bin/bash
# cocktail-idle-check.sh — Notification/idle_prompt hook (v3: server-side cursors)
# "Listen while you wait"
# Checks inbox during idle periods. Alerts on urgent messages.
#
# v3: Uses server-side read cursors via /api/cursors and /api/inbox.
#     No longer depends on file-based lastread (runtime-agnostic).

set -euo pipefail

# Read hook input from stdin
INPUT=$(cat)

# Derive agent ID from cwd
AGENT="${MING_QIAO_AGENT_ID:-}"
if [[ -z "$AGENT" ]]; then
    CWD=$(echo "$INPUT" | jq -r '.cwd // empty' 2>/dev/null)
    if [[ "$CWD" == *"/aleph"* ]]; then
        AGENT="aleph"
    elif [[ "$CWD" == *"/luban"* ]]; then
        AGENT="luban"
    fi
fi
if [[ -z "$AGENT" ]]; then
    exit 0
fi

MQ_URL="${MQ_URL:-http://localhost:7777}"

# Quick check: any unread?
CURSOR_RESPONSE=$(curl -s --connect-timeout 3 "$MQ_URL/api/cursors?agent=$AGENT" 2>/dev/null || echo "{}")
UNREAD_COUNT=$(echo "$CURSOR_RESPONSE" | jq -r '.cursors[0].unread_count // 0' 2>/dev/null || echo 0)

if [[ "$UNREAD_COUNT" -le 0 ]]; then
    exit 0
fi

# Fetch unread messages — only alert on request/discuss
INBOX_RESPONSE=$(curl -s --connect-timeout 3 "$MQ_URL/api/inbox/$AGENT?unread_only=true&peek=true&limit=20" 2>/dev/null || echo "{}")
URGENT=$(echo "$INBOX_RESPONSE" | jq -r '.messages[]? | select(.intent == "request" or .intent == "discuss") | "  From: \(.from // .from_agent) — \"\(.subject)\" (intent: \(.intent))"' 2>/dev/null || true)

if [[ -z "$URGENT" ]]; then
    exit 0
fi

URGENT_COUNT=$(echo "$URGENT" | wc -l | tr -d ' ')

jq -n --arg ctx "$(printf 'INTERRUPT: %d urgent message(s) arrived while idle:\n%s\nAction: Use check_messages to read and respond.' "$URGENT_COUNT" "$URGENT")" \
'{
  hookSpecificOutput: {
    hookEventName: "Notification",
    additionalContext: $ctx
  }
}'
