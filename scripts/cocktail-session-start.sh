#!/bin/bash
# cocktail-session-start.sh — SessionStart hook (v3: server-side cursors)
# "Know the room when you arrive"
# Injects pending notification summary as context at session start.
# Also sets MING_QIAO_AGENT_ID via CLAUDE_ENV_FILE for subsequent hooks/commands.
#
# v3: Uses server-side read cursors via /api/cursors and /api/inbox.
#     No longer depends on file-based lastread (runtime-agnostic).

set -euo pipefail

# Read hook input from stdin to get session context
INPUT=$(cat)

# Determine agent ID from the project directory
CWD=$(echo "$INPUT" | jq -r '.cwd // empty')
if [[ "$CWD" == *"/aleph"* ]]; then
    AGENT="aleph"
elif [[ "$CWD" == *"/luban"* ]]; then
    AGENT="luban"
elif [[ "$CWD" == *"/merlin"* ]]; then
    AGENT="merlin"
else
    AGENT="${MING_QIAO_AGENT_ID:-}"
fi

if [[ -z "$AGENT" ]]; then
    exit 0
fi

# Persist MING_QIAO_AGENT_ID for all subsequent Bash commands and hook scripts
if [[ -n "${CLAUDE_ENV_FILE:-}" ]]; then
    echo "export MING_QIAO_AGENT_ID=${AGENT}" >> "$CLAUDE_ENV_FILE"
fi

MQ_URL="${MQ_URL:-http://localhost:7777}"

# Query server-side cursor for unread count
CURSOR_RESPONSE=$(curl -s --connect-timeout 3 "$MQ_URL/api/cursors?agent=$AGENT" 2>/dev/null || echo "{}")
UNREAD_COUNT=$(echo "$CURSOR_RESPONSE" | jq -r '.cursors[0].unread_count // 0' 2>/dev/null || echo 0)

if [[ "$UNREAD_COUNT" -le 0 ]]; then
    jq -n --arg ctx "Session start: No new messages in inbox." \
    '{
      hookSpecificOutput: {
        hookEventName: "SessionStart",
        additionalContext: $ctx
      }
    }'
    exit 0
fi

# Fetch unread messages for categorization
INBOX_RESPONSE=$(curl -s --connect-timeout 3 "$MQ_URL/api/inbox/$AGENT?unread_only=true&peek=true&limit=50" 2>/dev/null || echo "{}")

REQUESTS=$(echo "$INBOX_RESPONSE" | jq -r '.messages[]? | select(.intent == "request") | "  REQUEST: From \(.from // .from_agent) — \"\(.subject)\""' 2>/dev/null || true)
DISCUSSIONS=$(echo "$INBOX_RESPONSE" | jq -r '.messages[]? | select(.intent == "discuss") | "  DISCUSS: From \(.from // .from_agent) — \"\(.subject)\""' 2>/dev/null || true)
INFORMS=$(echo "$INBOX_RESPONSE" | jq -r '.messages[]? | select(.intent == "inform") | "  INFORM: From \(.from // .from_agent) — \"\(.subject)\""' 2>/dev/null || true)

REQ_COUNT=0; DISC_COUNT=0; INF_COUNT=0
[[ -n "$REQUESTS" ]] && REQ_COUNT=$(echo "$REQUESTS" | wc -l | tr -d ' ')
[[ -n "$DISCUSSIONS" ]] && DISC_COUNT=$(echo "$DISCUSSIONS" | wc -l | tr -d ' ')
[[ -n "$INFORMS" ]] && INF_COUNT=$(echo "$INFORMS" | wc -l | tr -d ' ')

SUMMARY="Session start: ${UNREAD_COUNT} pending message(s) (${REQ_COUNT} request, ${DISC_COUNT} discuss, ${INF_COUNT} inform)"
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

# Do NOT advance cursor here — let the agent actually read via inbox API
