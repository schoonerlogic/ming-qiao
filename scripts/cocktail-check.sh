#!/bin/bash
# cocktail-check.sh — PostToolUse hook (v2)
# "Hear the room after every action — and KEEP hearing until you acknowledge"
#
# Design (per Thales):
# - NEVER advance lastread just because we displayed an alert
# - Only advance lastread when the agent proves they processed messages
#   (by calling read_inbox or check_messages via MCP, or curling the inbox API)
# - Re-alert on EVERY tool call for unacknowledged request messages
# - Include count, sender, time, and subject — make it impossible to ignore
#
# Outputs additionalContext JSON if unacknowledged messages exist.

set -euo pipefail

# Load shared security functions (path hardening, atomic writes, token stripping)
source "$(dirname "$0")/cocktail-lib.sh"

INPUT=$(cat)

# Derive agent ID from env or cwd (hardened path resolution)
AGENT="${MING_QIAO_AGENT_ID:-}"
if [[ -z "$AGENT" ]]; then
    CWD=$(echo "$INPUT" | jq -r '.cwd // empty' 2>/dev/null)
    if [[ -n "$CWD" ]]; then
        resolve_agent_id "$CWD" || true
    fi
fi
[[ -z "$AGENT" ]] && exit 0

NOTIFY_DIR="/Users/proteus/astralmaris/ming-qiao/notifications"

# Reject symlinked notification directory
if [[ -L "$NOTIFY_DIR" ]]; then
    exit 0
fi

INTERRUPT_FILE="$NOTIFY_DIR/${AGENT}.interrupt"
NOTIFY_FILE="$NOTIFY_DIR/${AGENT}.jsonl"
LASTREAD_FILE="$NOTIFY_DIR/${AGENT}.lastread"

# ── Check if this tool call acknowledges messages ──
# Agent proves processing by calling MCP inbox tools or curling the inbox API.
# Only THEN do we advance lastread.
TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty' 2>/dev/null)
TOOL_INPUT=$(echo "$INPUT" | jq -r '.tool_input // empty' 2>/dev/null)

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

if [[ "$ACKNOWLEDGED" == true && -f "$NOTIFY_FILE" ]]; then
    TOTAL_LINES=$(wc -l < "$NOTIFY_FILE" | tr -d ' ')
    atomic_write "$LASTREAD_FILE" "$TOTAL_LINES"
    exit 0  # Just acknowledged — no need to alert
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

# ── Check 2: Unacknowledged notifications (re-alerts every time) ──
if [[ -f "$NOTIFY_FILE" ]]; then
    TOTAL_LINES=$(wc -l < "$NOTIFY_FILE" | tr -d ' ')
    LAST_SEEN=0
    [[ -f "$LASTREAD_FILE" ]] && LAST_SEEN=$(cat "$LASTREAD_FILE" 2>/dev/null || echo 0)

    if [[ "$TOTAL_LINES" -gt "$LAST_SEEN" ]]; then
        NEW_LINES=$(tail -n +"$((LAST_SEEN + 1))" "$NOTIFY_FILE")

        # Extract request-intent messages with timestamp and subject
        REQUESTS=$(echo "$NEW_LINES" | jq -r '
            select(.intent == "request") |
            "  From: \(.from) (\(.timestamp | split("T")[1] | split(".")[0])). Subject: \(.subject)"
        ' 2>/dev/null || true)

        # Extract discuss-intent messages
        DISCUSS=$(echo "$NEW_LINES" | jq -r '
            select(.intent == "discuss") |
            "  From: \(.from) (\(.timestamp | split("T")[1] | split(".")[0])). Subject: \(.subject)"
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

        # DO NOT advance lastread here — only advance on acknowledgment
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
