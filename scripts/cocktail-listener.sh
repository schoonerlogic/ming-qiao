#!/bin/bash
# cocktail-listener.sh — Cocktail Party Protocol sidecar
# Watches notification JSONL, creates .interrupt for request/discuss intent
#
# Usage: ./scripts/cocktail-listener.sh <agent-id>
# Example: ./scripts/cocktail-listener.sh aleph &

set -euo pipefail

# Load shared security functions (path hardening, atomic writes, token stripping)
source "$(dirname "$0")/cocktail-lib.sh"

AGENT="${1:?Usage: cocktail-listener.sh <agent-id>}"

# Validate agent ID against known agents (no arbitrary path injection)
case "$AGENT" in
    aleph|luban|merlin|thales|ogma|laozi-jung|mataya) ;;
    *) echo "Unknown agent: $AGENT" >&2; exit 1 ;;
esac

NOTIFY_DIR="/Users/proteus/astralmaris/ming-qiao/notifications"
NOTIFY_FILE="${NOTIFY_DIR}/${AGENT}.jsonl"
INTERRUPT_FILE="${NOTIFY_DIR}/${AGENT}.interrupt"

# Verify notification directory is not a symlink
if [[ -L "$NOTIFY_DIR" ]]; then
    echo "ERROR: Notification directory is a symlink — refusing to proceed" >&2
    exit 1
fi

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
        # Strip any token material from subject/from before writing interrupt
        from=$(strip_tokens "$from")
        subject=$(strip_tokens "$subject")
        atomic_write "$INTERRUPT_FILE" "{\"from\":\"$from\",\"subject\":\"$subject\",\"intent\":\"$intent\",\"handled\":\"pending\"}"
        echo "[$(date '+%H:%M:%S')] Interrupt created: $from -> $AGENT ($intent: $subject)"
    fi
done
