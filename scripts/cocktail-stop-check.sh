#!/bin/bash
# cocktail-stop-check.sh — Stop hook (v3: server-side cursors)
# "Don't stop if someone is talking to you"
# Blocks the agent from stopping if there are unread request-intent messages.
# Exit code 2 = block stop. Exit code 0 = allow stop.
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
    elif [[ "$CWD" == *"/merlin"* ]]; then
        AGENT="merlin"
    fi
fi
if [[ -z "$AGENT" ]]; then
    exit 0
fi

MQ_URL="${MQ_URL:-http://localhost:7777}"

# Quick check: any unread at all?
CURSOR_RESPONSE=$(curl -s --connect-timeout 3 "$MQ_URL/api/cursors?agent=$AGENT" 2>/dev/null || echo "{}")
UNREAD_COUNT=$(echo "$CURSOR_RESPONSE" | jq -r '.cursors[0].unread_count // 0' 2>/dev/null || echo 0)

if [[ "$UNREAD_COUNT" -le 0 ]]; then
    exit 0  # All caught up, ok to stop
fi

# Check for unread request-intent messages only (discuss can wait)
INBOX_RESPONSE=$(curl -s --connect-timeout 3 "$MQ_URL/api/inbox/$AGENT?unread_only=true&peek=true&limit=50" 2>/dev/null || echo "{}")
REQUESTS=$(echo "$INBOX_RESPONSE" | jq -r '.messages[]? | select(.intent == "request") | "  From: \(.from // .from_agent) — \"\(.subject)\""' 2>/dev/null || true)

if [[ -z "$REQUESTS" ]]; then
    exit 0  # No pending requests, ok to stop
fi

REQUEST_COUNT=$(echo "$REQUESTS" | wc -l | tr -d ' ')

# Block the stop — exit code 2 with stderr message
echo "You have ${REQUEST_COUNT} unread request-intent message(s). Handle them before stopping." >&2
echo "Pending requests:" >&2
echo "$REQUESTS" >&2
echo "Use check_messages with unread_only=true to see pending messages." >&2
exit 2
