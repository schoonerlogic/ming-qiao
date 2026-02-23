#!/bin/bash
# cocktail-listener.sh — Cocktail Party Protocol sidecar
# Watches notification JSONL, creates .interrupt for request/discuss intent
#
# Usage: ./scripts/cocktail-listener.sh <agent-id>
# Example: ./scripts/cocktail-listener.sh aleph &

set -euo pipefail

AGENT="${1:?Usage: cocktail-listener.sh <agent-id>}"
NOTIFY_FILE="/Users/proteus/astralmaris/ming-qiao/notifications/${AGENT}.jsonl"
INTERRUPT_FILE="/Users/proteus/astralmaris/ming-qiao/notifications/${AGENT}.interrupt"

if [[ ! -f "$NOTIFY_FILE" ]]; then
    echo "Notification file not found: $NOTIFY_FILE"
    echo "Creating empty file..."
    touch "$NOTIFY_FILE"
fi

echo "Cocktail listener started for agent: $AGENT"
echo "Watching: $NOTIFY_FILE"
echo "Interrupt: $INTERRUPT_FILE"

tail -f -n0 "$NOTIFY_FILE" | while read -r line; do
    intent=$(echo "$line" | jq -r '.intent // empty')
    if [[ "$intent" == "request" || "$intent" == "discuss" ]]; then
        from=$(echo "$line" | jq -r '.from // "unknown"')
        subject=$(echo "$line" | jq -r '.subject // "no subject"')
        echo "[URGENT] From: $from — $subject (intent: $intent)" > "$INTERRUPT_FILE"
        echo "[$(date '+%H:%M:%S')] Interrupt created: $from -> $AGENT ($intent: $subject)"
    fi
done
