#!/usr/bin/env bash
# mq-inbox.sh — Read your ming-qiao inbox
# Usage: mq-inbox.sh [--count N] [--raw]
#
# Environment:
#   MQ_AGENT  — Your agent ID (default: auto-detect from repo worktree name)
#   MQ_URL    — Ming-qiao base URL (default: http://localhost:7777)
#
# Examples:
#   mq-inbox.sh              # Show inbox summary
#   mq-inbox.sh --count 5    # Show last 5 messages
#   mq-inbox.sh --raw        # Output raw JSON

set -euo pipefail

MQ_URL="${MQ_URL:-http://localhost:7777}"

# Auto-detect agent ID from git worktree name if not set
if [ -z "${MQ_AGENT:-}" ]; then
    worktree_dir="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
    MQ_AGENT="$(basename "$worktree_dir")"
fi

COUNT=""
RAW=false

while [ $# -gt 0 ]; do
    case "$1" in
        --count) COUNT="$2"; shift 2 ;;
        --raw) RAW=true; shift ;;
        --help|-h)
            echo "Usage: mq-inbox.sh [--count N] [--raw]"
            echo ""
            echo "  --count N  Show only the last N messages"
            echo "  --raw      Output raw JSON"
            echo ""
            echo "Environment:"
            echo "  MQ_AGENT=$MQ_AGENT"
            echo "  MQ_URL=$MQ_URL"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

RESPONSE=$(curl -s -w "\n%{http_code}" "${MQ_URL}/api/inbox/${MQ_AGENT}")

HTTP_CODE=$(echo "$RESPONSE" | tail -1)
BODY=$(echo "$RESPONSE" | sed '$d')

if [ "$HTTP_CODE" -ge 200 ] && [ "$HTTP_CODE" -lt 300 ]; then
    if [ "$RAW" = true ]; then
        echo "$BODY"
        exit 0
    fi

    python3 -c "
import json, sys

data = json.loads(sys.argv[1])
count = int(sys.argv[2]) if sys.argv[2] else 0
messages = data.get('messages', [])
total = data.get('total_count', len(messages))
unread = data.get('unread_count', 0)

print(f'Inbox for ${MQ_AGENT}: {total} total, {unread} unread')
print('=' * 60)

if count > 0:
    messages = messages[:count]

for msg in messages:
    intent = msg.get('intent', '?')
    marker = {'request': '!', 'discuss': '?', 'inform': 'i'}.get(intent, ' ')
    frm = msg.get('from', '?')
    subj = msg.get('subject', '(no subject)')
    ts = msg.get('timestamp', '')[:16]
    print(f'  [{marker}] {ts}  {frm:>12}  {subj}')

if not messages:
    print('  (empty)')
" "$BODY" "${COUNT:-0}"
else
    echo "ERROR ($HTTP_CODE): Failed to read inbox for $MQ_AGENT" >&2
    echo "$BODY" >&2
    exit 1
fi
