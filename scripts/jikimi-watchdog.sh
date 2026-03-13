#!/bin/bash
# jikimi-watchdog.sh — Separate watchdog for jikimi-healthd (Ogma J-R4)
# Runs on its own launchd schedule (every 2 minutes)
# Checks daemon heartbeat file — if stale (>90s), alerts and attempts restart

set -uo pipefail

HEARTBEAT_FILE="/Users/proteus/astralmaris/ming-qiao/jikimi/health/.heartbeat"
PID_FILE="/Users/proteus/astralmaris/ming-qiao/jikimi/health/.jikimi-healthd.pid"
MQ_URL="http://localhost:7777"
MQ_TOKEN="mq-jikimi-13ebd293892ef79ba69cad74858a7297"
STALE_THRESHOLD=90  # seconds

ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)

alert() {
    local message="$1"
    local severity="${2:-high}"

    # Try ming-qiao
    curl -s --connect-timeout 3 --max-time 5 \
        -X POST "${MQ_URL}/api/threads" \
        -H "Content-Type: application/json" \
        -H "Authorization: Bearer ${MQ_TOKEN}" \
        -d "$(printf '{"from":"jikimi","to":"aleph","subject":"WATCHDOG: jikimi-healthd issue","content":"%s","intent":"inform","priority":"%s","synthetic":true}' \
            "$message" "$severity")" >/dev/null 2>&1

    # Always write fallback too
    local pending="/Users/proteus/astralmaris/ming-qiao/aleph/PENDING_MESSAGES.md"
    {
        echo ""
        echo "## WATCHDOG ALERT — jikimi-healthd [${severity}]"
        echo "**From:** jikimi-watchdog"
        echo "**Time:** ${ts}"
        echo ""
        echo "${message}"
        echo ""
        echo "---"
    } >> "$pending"
}

# Check heartbeat file exists
if [[ ! -f "$HEARTBEAT_FILE" ]]; then
    alert "Heartbeat file missing — jikimi-healthd may never have started"
    exit 1
fi

# Check heartbeat age
heartbeat_epoch=$(stat -f %m "$HEARTBEAT_FILE")
now_epoch=$(date +%s)
age=$(( now_epoch - heartbeat_epoch ))

if [[ "$age" -gt "$STALE_THRESHOLD" ]]; then
    alert "Heartbeat stale (${age}s old, threshold ${STALE_THRESHOLD}s). Daemon may be hung or dead."

    # Check if PID is still alive
    if [[ -f "$PID_FILE" ]]; then
        daemon_pid=$(cat "$PID_FILE")
        if kill -0 "$daemon_pid" 2>/dev/null; then
            alert "Daemon PID ${daemon_pid} exists but heartbeat stale — possible hang" "critical"
        else
            alert "Daemon PID ${daemon_pid} is dead — launchd should restart it" "high"
        fi
    fi
fi
