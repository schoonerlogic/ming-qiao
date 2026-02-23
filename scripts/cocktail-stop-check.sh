#!/bin/bash
# cocktail-stop-check.sh — Stop hook
# "Don't stop if someone is talking to you"
# Blocks the agent from stopping if there are unread request-intent messages.
# Exit code 2 = block stop. Exit code 0 = allow stop.

set -euo pipefail

# Read hook input from stdin
INPUT=$(cat)

# Derive agent ID from cwd — CLAUDE_ENV_FILE vars don't reach hook subprocesses
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

NOTIFY_FILE="/Users/proteus/astralmaris/ming-qiao/notifications/${AGENT}.jsonl"
LASTREAD_FILE="/Users/proteus/astralmaris/ming-qiao/notifications/${AGENT}.lastread"

if [[ ! -f "$NOTIFY_FILE" ]]; then
    exit 0
fi

TOTAL_LINES=$(wc -l < "$NOTIFY_FILE" | tr -d ' ')

LAST_SEEN=0
if [[ -f "$LASTREAD_FILE" ]]; then
    LAST_SEEN=$(cat "$LASTREAD_FILE" 2>/dev/null || echo 0)
fi

if [[ "$TOTAL_LINES" -le "$LAST_SEEN" ]]; then
    exit 0  # All caught up, ok to stop
fi

# Check for unread request-intent messages only (discuss can wait)
NEW_LINES=$(tail -n +"$((LAST_SEEN + 1))" "$NOTIFY_FILE")
REQUESTS=$(echo "$NEW_LINES" | jq -r 'select(.intent == "request") | "  From: \(.from) — \"\(.subject)\"" ' 2>/dev/null || true)

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
