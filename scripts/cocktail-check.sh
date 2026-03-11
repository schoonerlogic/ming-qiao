#!/bin/bash
# cocktail-check.sh — PostToolUse hook (v3: server-side cursors)
# "Hear the room after every action — and KEEP hearing until you acknowledge"
#
# Design (per Thales):
# - NEVER advance read cursor just because we displayed an alert
# - Only advance when the agent proves they processed messages
#   (by calling read_inbox or check_messages via MCP, or curling the inbox API)
# - Re-alert on EVERY tool call for unacknowledged request messages
# - Include count, sender, time, and subject — make it impossible to ignore
#
# v3: Uses server-side read cursors via /api/inbox and /api/cursors.
#     No longer depends on file-based lastread (runtime-agnostic).

set -euo pipefail

INPUT=$(cat)

# Derive agent ID from env or cwd
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
[[ -z "$AGENT" ]] && exit 0

MQ_URL="${MQ_URL:-http://localhost:7777}"
NOTIFY_DIR="/Users/proteus/astralmaris/ming-qiao/notifications"
INTERRUPT_FILE="$NOTIFY_DIR/${AGENT}.interrupt"

# ── Check if this tool call acknowledges messages ──
# Agent proves processing by calling MCP inbox tools or curling the inbox API.
# The inbox API auto-advances the server-side cursor on read.
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty' 2>/dev/null)

ACKNOWLEDGED=false

# MCP tool acknowledgment: tool name contains inbox/messages keywords
case "$TOOL_NAME" in
    *read_inbox*|*check_messages*|*tool_read_inbox*)
        ACKNOWLEDGED=true
        ;;
esac

# Bash/curl acknowledgment: command hits the inbox API endpoint
if [[ "$ACKNOWLEDGED" == false && "$TOOL_NAME" == "Bash" ]]; then
    COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty' 2>/dev/null)
    if [[ "$COMMAND" == *"/api/inbox/"* ]]; then
        ACKNOWLEDGED=true
    fi
fi

# If acknowledged, the API auto-advanced the cursor — nothing to do
if [[ "$ACKNOWLEDGED" == true ]]; then
    # Clean up interrupt file if present
    rm -f "$INTERRUPT_FILE"
    exit 0
fi

CONTEXT=""

# ── Check 1: Interrupt file from background handler ──
if [[ -f "$INTERRUPT_FILE" ]]; then
    INT_DATA=$(cat "$INTERRUPT_FILE")
    INT_FROM=$(echo "$INT_DATA" | jq -r '.from // "unknown"' 2>/dev/null)
    INT_SUBJECT=$(echo "$INT_DATA" | jq -r '.subject // ""' 2>/dev/null)
    INT_HANDLED=$(echo "$INT_DATA" | jq -r '.handled // "pending"' 2>/dev/null)

    if [[ "$INT_HANDLED" == "complete" ]]; then
        CONTEXT="BACKGROUND UPDATE: A message from $INT_FROM (re: \"$INT_SUBJECT\") was handled by a background session. Call read_inbox or check_messages to acknowledge."
    elif [[ "$INT_HANDLED" == "pending" ]]; then
        CONTEXT="INCOMING: A message from $INT_FROM (re: \"$INT_SUBJECT\") is being handled by a background session. Continue your current work."
    fi

    rm -f "$INTERRUPT_FILE"
fi

# ── Check 2: Unread messages via server-side cursor ──
CURSOR_RESPONSE=$(curl -s --connect-timeout 3 "$MQ_URL/api/cursors?agent=$AGENT" 2>/dev/null || echo "{}")
UNREAD_COUNT=$(echo "$CURSOR_RESPONSE" | jq -r '.cursors[0].unread_count // 0' 2>/dev/null || echo 0)

if [[ "$UNREAD_COUNT" -gt 0 ]]; then
    # Fetch unread messages for detail
    INBOX_RESPONSE=$(curl -s --connect-timeout 3 "$MQ_URL/api/inbox/$AGENT?unread_only=true&peek=true&limit=20" 2>/dev/null || echo "{}")

    # Extract request-intent messages
    REQUESTS=$(echo "$INBOX_RESPONSE" | jq -r '
        .messages[]? | select(.intent == "request") |
        "  From: \(.from // .from_agent) (\((.timestamp // .created_at) | split("T")[1] | split(".")[0])). Subject: \(.subject)"
    ' 2>/dev/null || true)

    # Extract discuss-intent messages
    DISCUSS=$(echo "$INBOX_RESPONSE" | jq -r '
        .messages[]? | select(.intent == "discuss") |
        "  From: \(.from // .from_agent) (\((.timestamp // .created_at) | split("T")[1] | split(".")[0])). Subject: \(.subject)"
    ' 2>/dev/null || true)

    UNACKED_CTX=""

    if [[ -n "$REQUESTS" ]]; then
        REQ_COUNT=$(echo "$REQUESTS" | wc -l | tr -d ' ')
        UNACKED_CTX=$(printf 'You have %d UNACKNOWLEDGED REQUEST(s). You MUST call read_inbox or check_messages NOW:\n%s' "$REQ_COUNT" "$REQUESTS")
    fi

    if [[ -n "$DISCUSS" ]]; then
        DISC_COUNT=$(echo "$DISCUSS" | wc -l | tr -d ' ')
        if [[ -n "$UNACKED_CTX" ]]; then
            UNACKED_CTX=$(printf '%s\nAlso %d discuss message(s):\n%s' "$UNACKED_CTX" "$DISC_COUNT" "$DISCUSS")
        else
            UNACKED_CTX=$(printf '%d unacknowledged discuss message(s):\n%s\nCall read_inbox or check_messages to acknowledge.' "$DISC_COUNT" "$DISCUSS")
        fi
    fi

    if [[ -n "$UNACKED_CTX" ]]; then
        if [[ -n "$CONTEXT" ]]; then
            CONTEXT="$CONTEXT | $UNACKED_CTX"
        else
            CONTEXT="$UNACKED_CTX"
        fi
    fi
fi

# ── Output ──
if [[ -n "$CONTEXT" ]]; then
    jq -n --arg ctx "$CONTEXT" '{
      systemMessage: $ctx,
      hookSpecificOutput: {
        hookEventName: "PostToolUse",
        additionalContext: $ctx
      }
    }'
fi
