#!/usr/bin/env bash
# mq-send.sh — Send a message via ming-qiao HTTP API
# Usage: mq-send.sh <to> <subject> <message> [--intent request|discuss|inform]
#
# Environment:
#   MQ_AGENT  — Your agent ID (default: auto-detect from repo worktree name)
#   MQ_URL    — Ming-qiao base URL (default: http://localhost:7777)
#   MQ_TOKEN  — Bearer token for API auth (auto-loaded from token file if available)
#
# Examples:
#   mq-send.sh aleph "Need review" "Please review my changes"
#   mq-send.sh council "Status update" "Work complete" --intent inform

set -euo pipefail

MQ_URL="${MQ_URL:-http://localhost:7777}"

# Auto-detect agent ID from git worktree name if not set
if [ -z "${MQ_AGENT:-}" ]; then
    worktree_dir="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
    MQ_AGENT="$(basename "$worktree_dir")"
fi

if [ $# -lt 3 ]; then
    echo "Usage: mq-send.sh <to> <subject> <message> [--intent request|discuss|inform]"
    echo ""
    echo "  to       Recipient agent ID (aleph, thales, luban, council, etc.)"
    echo "  subject  Message subject line"
    echo "  message  Message body"
    echo ""
    echo "Options:"
    echo "  --intent   Message intent: request, discuss, or inform (default: inform)"
    echo ""
    echo "Environment:"
    echo "  MQ_AGENT=$MQ_AGENT"
    echo "  MQ_URL=$MQ_URL"
    exit 1
fi

TO="$1"
SUBJECT="$2"
MESSAGE="$3"
INTENT="inform"

shift 3
while [ $# -gt 0 ]; do
    case "$1" in
        --intent) INTENT="$2"; shift 2 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# Build JSON payload using python3 for safe escaping
PAYLOAD=$(python3 -c "
import json, sys
print(json.dumps({
    'from': sys.argv[1],
    'to': sys.argv[2],
    'subject': sys.argv[3],
    'content': sys.argv[4],
    'intent': sys.argv[5]
}))
" "$MQ_AGENT" "$TO" "$SUBJECT" "$MESSAGE" "$INTENT")

# Auto-load bearer token from token file if MQ_TOKEN not set
if [ -z "${MQ_TOKEN:-}" ]; then
    TOKENS_FILE="$(cd "$(dirname "$0")/.." && pwd)/config/agent-tokens.json"
    if [ -f "$TOKENS_FILE" ]; then
        MQ_TOKEN=$(python3 -c "import json; d=json.load(open('$TOKENS_FILE')); print(d['tokens'].get('$MQ_AGENT',''))" 2>/dev/null || true)
    fi
fi

CURL_ARGS=(-s -w "\n%{http_code}" -X POST "${MQ_URL}/api/threads" -H "Content-Type: application/json")
if [ -n "${MQ_TOKEN:-}" ]; then
    CURL_ARGS+=(-H "Authorization: Bearer ${MQ_TOKEN}")
fi
CURL_ARGS+=(-d "$PAYLOAD")

RESPONSE=$(curl "${CURL_ARGS[@]}")

HTTP_CODE=$(echo "$RESPONSE" | tail -1)
BODY=$(echo "$RESPONSE" | sed '$d')

if [ "$HTTP_CODE" -ge 200 ] && [ "$HTTP_CODE" -lt 300 ]; then
    echo "Sent to $TO: $SUBJECT"
    echo "$BODY" | python3 -m json.tool 2>/dev/null || echo "$BODY"
else
    echo "ERROR ($HTTP_CODE): Failed to send message" >&2
    echo "$BODY" >&2
    exit 1
fi
