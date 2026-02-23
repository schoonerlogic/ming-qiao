#!/bin/bash
# cocktail-check.sh — PostToolUse hook
# "Hear the room after every action"
# Checks notification JSONL for unread request/discuss messages since last check.
# Outputs additionalContext JSON if urgent messages are pending.

set -euo pipefail

# Read hook input from stdin (contains cwd, tool_name, etc.)
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
    exit 0  # Cannot determine agent ID, skip silently
fi

NOTIFY_FILE="/Users/proteus/astralmaris/ming-qiao/notifications/${AGENT}.jsonl"
LASTREAD_FILE="/Users/proteus/astralmaris/ming-qiao/notifications/${AGENT}.lastread"

if [[ ! -f "$NOTIFY_FILE" ]]; then
    exit 0
fi

TOTAL_LINES=$(wc -l < "$NOTIFY_FILE" | tr -d ' ')

# Read last-seen position
LAST_SEEN=0
if [[ -f "$LASTREAD_FILE" ]]; then
    LAST_SEEN=$(cat "$LASTREAD_FILE" 2>/dev/null || echo 0)
fi

# No new lines
if [[ "$TOTAL_LINES" -le "$LAST_SEEN" ]]; then
    exit 0
fi

# Extract new lines and filter for request/discuss intent
NEW_LINES=$(tail -n +"$((LAST_SEEN + 1))" "$NOTIFY_FILE")
URGENT=$(echo "$NEW_LINES" | jq -r 'select(.intent == "request" or .intent == "discuss") | "  From: \(.from) — \"\(.subject)\" (intent: \(.intent))"' 2>/dev/null || true)

if [[ -z "$URGENT" ]]; then
    # Update position — we saw the messages, they're just not urgent
    echo "$TOTAL_LINES" > "$LASTREAD_FILE"
    exit 0
fi

URGENT_COUNT=$(echo "$URGENT" | wc -l | tr -d ' ')

# Output additionalContext JSON for Claude
jq -n --arg ctx "$(printf '⚠️ INTERRUPT: %d urgent message(s) waiting:\n%s\nAction: Use check_messages to read and respond BEFORE continuing your current work.' "$URGENT_COUNT" "$URGENT")" \
'{
  hookSpecificOutput: {
    hookEventName: "PostToolUse",
    additionalContext: $ctx
  }
}'

# Update position
echo "$TOTAL_LINES" > "$LASTREAD_FILE"
