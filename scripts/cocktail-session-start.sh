#!/bin/bash
# cocktail-session-start.sh — SessionStart hook
# "Know the room when you arrive"
# Injects pending notification summary as context at session start.
# Also sets MING_QIAO_AGENT_ID via CLAUDE_ENV_FILE for subsequent hooks/commands.

set -euo pipefail

# Load shared security functions (path hardening, atomic writes, token stripping)
source "$(dirname "$0")/cocktail-lib.sh"

# Read hook input from stdin to get session context
INPUT=$(cat)

# Determine agent ID from the project directory (hardened path resolution)
CWD=$(echo "$INPUT" | jq -r '.cwd // empty')
AGENT="${MING_QIAO_AGENT_ID:-}"
if [[ -z "$AGENT" ]]; then
    if [[ -n "$CWD" ]]; then
        resolve_agent_id "$CWD" || true
    fi
fi

if [[ -z "$AGENT" ]]; then
    exit 0
fi

# Persist MING_QIAO_AGENT_ID for all subsequent Bash commands and hook scripts
if [[ -n "${CLAUDE_ENV_FILE:-}" ]]; then
    echo "export MING_QIAO_AGENT_ID=${AGENT}" >> "$CLAUDE_ENV_FILE"
fi

NOTIFY_DIR="/Users/proteus/astralmaris/ming-qiao/notifications"
NOTIFY_FILE="${NOTIFY_DIR}/${AGENT}.jsonl"
LASTREAD_FILE="${NOTIFY_DIR}/${AGENT}.lastread"

# Reject symlinked notification directory
if [[ -L "$NOTIFY_DIR" ]]; then
    exit 0
fi

if [[ ! -f "$NOTIFY_FILE" ]]; then
    exit 0
fi

TOTAL_LINES=$(wc -l < "$NOTIFY_FILE" | tr -d ' ')

LAST_SEEN=0
if [[ -f "$LASTREAD_FILE" ]]; then
    LAST_SEEN=$(cat "$LASTREAD_FILE" 2>/dev/null || echo 0)
fi

if [[ "$TOTAL_LINES" -le "$LAST_SEEN" ]]; then
    # No new messages
    jq -n --arg ctx "Session start: No new messages in inbox." \
    '{
      hookSpecificOutput: {
        hookEventName: "SessionStart",
        additionalContext: $ctx
      }
    }'
    exit 0
fi

# Categorize new messages
NEW_LINES=$(tail -n +"$((LAST_SEEN + 1))" "$NOTIFY_FILE")
REQUESTS=$(echo "$NEW_LINES" | jq -r 'select(.intent == "request") | "  REQUEST: From \(.from) — \"\(.subject)\"" ' 2>/dev/null || true)
DISCUSSIONS=$(echo "$NEW_LINES" | jq -r 'select(.intent == "discuss") | "  DISCUSS: From \(.from) — \"\(.subject)\"" ' 2>/dev/null || true)
INFORMS=$(echo "$NEW_LINES" | jq -r 'select(.intent == "inform") | "  INFORM: From \(.from) — \"\(.subject)\"" ' 2>/dev/null || true)

NEW_COUNT=$((TOTAL_LINES - LAST_SEEN))
REQ_COUNT=0; DISC_COUNT=0; INF_COUNT=0
[[ -n "$REQUESTS" ]] && REQ_COUNT=$(echo "$REQUESTS" | wc -l | tr -d ' ')
[[ -n "$DISCUSSIONS" ]] && DISC_COUNT=$(echo "$DISCUSSIONS" | wc -l | tr -d ' ')
[[ -n "$INFORMS" ]] && INF_COUNT=$(echo "$INFORMS" | wc -l | tr -d ' ')

SUMMARY="Session start: ${NEW_COUNT} pending message(s) (${REQ_COUNT} request, ${DISC_COUNT} discuss, ${INF_COUNT} inform)"
[[ -n "$REQUESTS" ]] && SUMMARY="$SUMMARY"$'\n'"$REQUESTS"
[[ -n "$DISCUSSIONS" ]] && SUMMARY="$SUMMARY"$'\n'"$DISCUSSIONS"
[[ -n "$INFORMS" ]] && SUMMARY="$SUMMARY"$'\n'"$INFORMS"
SUMMARY="$SUMMARY"$'\n'"Use check_messages to review and respond to pending messages."

jq -n --arg ctx "$SUMMARY" \
'{
  hookSpecificOutput: {
    hookEventName: "SessionStart",
    additionalContext: $ctx
  }
}'

# Do NOT update lastread here — let the agent actually read the messages first
