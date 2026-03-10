#!/usr/bin/env bash
# wake-agent.sh — Phase 0 Agent Wake Daemon (JSONL polling)
# Polls an agent's notification JSONL for wake-worthy messages
# and spawns a headless claude -p session to process them.
#
# Usage: ./wake-agent.sh <agent-id> [poll-interval-seconds]
# Example: ./wake-agent.sh aleph 30
#          ./wake-agent.sh luban 30
#
# Run in background: nohup ./wake-agent.sh aleph 30 &

set -euo pipefail

AGENT="${1:?Usage: wake-agent.sh <agent-id> [poll-seconds]}"
POLL_INTERVAL="${2:-30}"

NOTIFY_FILE="/Users/proteus/astralmaris/ming-qiao/notifications/${AGENT}.jsonl"
LASTREAD_FILE="/Users/proteus/astralmaris/ming-qiao/notifications/${AGENT}.wake-lastread"
LOG_DIR="/Users/proteus/astralmaris/ming-qiao/logs"
WAKE_LOG="${LOG_DIR}/wake-${AGENT}.log"
COOLDOWN_FILE="/tmp/astralmaris-wake-${AGENT}.cooldown"
COOLDOWN_SECONDS=300  # 5 minute cooldown after wake
MAX_DAILY_WAKES=20

# Agent working directories
case "$AGENT" in
    aleph) AGENT_DIR="/Users/proteus/astralmaris/astral-forge" ;;
    luban) AGENT_DIR="/Users/proteus/astralmaris/inference-kitchen" ;;
    *)
        echo "Unknown agent: $AGENT" >&2
        exit 1
        ;;
esac

mkdir -p "$LOG_DIR"

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" | tee -a "$WAKE_LOG"
}

is_agent_active() {
    # Check for running claude process in agent's directory
    pgrep -f "claude.*$(basename "$AGENT_DIR")" > /dev/null 2>&1
}

is_cooling_down() {
    if [[ ! -f "$COOLDOWN_FILE" ]]; then
        return 1
    fi
    local cooldown_until
    cooldown_until=$(cat "$COOLDOWN_FILE" 2>/dev/null || echo 0)
    local now
    now=$(date +%s)
    [[ "$now" -lt "$cooldown_until" ]]
}

set_cooldown() {
    echo $(( $(date +%s) + COOLDOWN_SECONDS )) > "$COOLDOWN_FILE"
}

daily_wake_count() {
    local today
    today=$(date '+%Y-%m-%d')
    grep -c "WAKE_SPAWN.*${today}" "$WAKE_LOG" 2>/dev/null || echo 0
}

wake_agent() {
    local trigger_from="$1"
    local trigger_subject="$2"
    local trigger_er="$3"
    local trigger_id="$4"

    log "WAKE_SPAWN agent=$AGENT trigger_from=$trigger_from er=$trigger_er id=$trigger_id"

    cd "$AGENT_DIR"
    claude -p \
        "WAKE: You have urgent messages on ming-qiao. Use check_messages to read your inbox. Process any messages with expected_response=reply or expected_response=comply. Respond to each appropriately using send_message. When done, briefly summarize what you handled." \
        --continue \
        --max-turns 15 \
        --output-format json \
        >> "${LOG_DIR}/${AGENT}-wake-sessions.jsonl" 2>&1

    local exit_code=$?
    log "WAKE_COMPLETE agent=$AGENT exit_code=$exit_code"
    set_cooldown
}

# --- Main Loop ---

log "Wake daemon started for agent=$AGENT poll=${POLL_INTERVAL}s cooldown=${COOLDOWN_SECONDS}s"
log "Watching: $NOTIFY_FILE"
log "Agent dir: $AGENT_DIR"

# Initialize lastread marker
if [[ ! -f "$LASTREAD_FILE" ]]; then
    if [[ -f "$NOTIFY_FILE" ]]; then
        wc -l < "$NOTIFY_FILE" | tr -d ' ' > "$LASTREAD_FILE"
    else
        echo 0 > "$LASTREAD_FILE"
    fi
    log "Initialized lastread at line $(cat "$LASTREAD_FILE")"
fi

while true; do
    sleep "$POLL_INTERVAL"

    # Skip if notification file doesn't exist
    if [[ ! -f "$NOTIFY_FILE" ]]; then
        continue
    fi

    TOTAL_LINES=$(wc -l < "$NOTIFY_FILE" | tr -d ' ')
    LAST_SEEN=$(cat "$LASTREAD_FILE" 2>/dev/null || echo 0)

    # No new messages
    if [[ "$TOTAL_LINES" -le "$LAST_SEEN" ]]; then
        continue
    fi

    # Check new messages for wake-worthy expected_response
    WAKE_MESSAGES=$(tail -n +"$((LAST_SEEN + 1))" "$NOTIFY_FILE" \
        | jq -r 'select(.expected_response == "reply" or .expected_response == "comply") | "\(.from)\t\(.subject // "no subject")\t\(.expected_response)\t\(.event_id // "no-id")"' \
        2>/dev/null || true)

    # Update lastread regardless (don't re-process old messages)
    echo "$TOTAL_LINES" > "$LASTREAD_FILE"

    if [[ -z "$WAKE_MESSAGES" ]]; then
        continue
    fi

    WAKE_COUNT=$(echo "$WAKE_MESSAGES" | wc -l | tr -d ' ')
    log "WAKE_TRIGGER $WAKE_COUNT wake-worthy message(s) detected"

    # Check guards
    if is_agent_active; then
        log "WAKE_SKIP agent is already active (session running)"
        continue
    fi

    if is_cooling_down; then
        log "WAKE_SKIP cooling down (recent wake within ${COOLDOWN_SECONDS}s)"
        continue
    fi

    DAILY=$(daily_wake_count)
    if [[ "$DAILY" -ge "$MAX_DAILY_WAKES" ]]; then
        log "WAKE_SKIP daily budget exhausted ($DAILY/$MAX_DAILY_WAKES)"
        continue
    fi

    # Extract first trigger for logging
    FIRST_FROM=$(echo "$WAKE_MESSAGES" | head -1 | cut -f1)
    FIRST_SUBJ=$(echo "$WAKE_MESSAGES" | head -1 | cut -f2)
    FIRST_ER=$(echo "$WAKE_MESSAGES" | head -1 | cut -f3)
    FIRST_ID=$(echo "$WAKE_MESSAGES" | head -1 | cut -f4)

    wake_agent "$FIRST_FROM" "$FIRST_SUBJ" "$FIRST_ER" "$FIRST_ID"
done
