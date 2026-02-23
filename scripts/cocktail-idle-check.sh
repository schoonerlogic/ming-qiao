#!/bin/bash
# cocktail-idle-check.sh — Notification/idle_prompt hook
# "Listen while you wait"
# Checks inbox during idle periods. Same logic as cocktail-check.sh
# but with Notification-specific output.

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
    exit 0
fi

NEW_LINES=$(tail -n +"$((LAST_SEEN + 1))" "$NOTIFY_FILE")
URGENT=$(echo "$NEW_LINES" | jq -r 'select(.intent == "request" or .intent == "discuss") | "  From: \(.from) — \"\(.subject)\" (intent: \(.intent))"' 2>/dev/null || true)

if [[ -z "$URGENT" ]]; then
    echo "$TOTAL_LINES" > "$LASTREAD_FILE"
    exit 0
fi

URGENT_COUNT=$(echo "$URGENT" | wc -l | tr -d ' ')

jq -n --arg ctx "$(printf '⚠️ INTERRUPT: %d urgent message(s) arrived while idle:\n%s\nAction: Use check_messages to read and respond.' "$URGENT_COUNT" "$URGENT")" \
'{
  hookSpecificOutput: {
    hookEventName: "Notification",
    additionalContext: $ctx
  }
}'

echo "$TOTAL_LINES" > "$LASTREAD_FILE"
