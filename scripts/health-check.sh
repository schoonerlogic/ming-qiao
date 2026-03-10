#!/usr/bin/env bash
# health-check.sh — Check all 6 Council infrastructure services
# Exit 0 if all healthy, 1 if any degraded.
# Posts alert to ming-qiao council thread if any service is down.
#
# Usage: health-check.sh [--quiet] [--no-alert]

set -euo pipefail

QUIET=false
ALERT=true
for arg in "$@"; do
    case "${arg}" in
        --quiet) QUIET=true ;;
        --no-alert) ALERT=false ;;
    esac
done

DEGRADED=0
STATUS_LINES=()

check_service() {
    local name="$1"
    local check_cmd="$2"

    if eval "${check_cmd}" > /dev/null 2>&1; then
        STATUS_LINES+=("OK  ${name}")
    else
        STATUS_LINES+=("DOWN  ${name}")
        DEGRADED=$((DEGRADED + 1))
    fi
}

# 1. Ming-Qiao HTTP API
check_service "ming-qiao (port 7777)" \
    "curl -sf --max-time 5 http://localhost:7777/api/config"

# 2. SurrealDB
check_service "SurrealDB (port 8000)" \
    "curl -sf --max-time 5 http://localhost:8000/health"

# 3. Graphiti MCP
check_service "Graphiti MCP (port 8001)" \
    "curl -sf --max-time 5 http://localhost:8001/health"

# 4. FalkorDB
check_service "FalkorDB (port 6379)" \
    "docker exec docker-falkordb-1 redis-cli -p 6379 ping"

# 5. Ollama
check_service "Ollama (port 11434)" \
    "curl -sf --max-time 5 http://localhost:11434/api/tags"

# 6. NATS (check client port — monitoring port 8222 not enabled)
check_service "NATS (port 4222)" \
    "echo '' | nc -w 2 localhost 4222"

# Print results
if [[ "${QUIET}" == false ]]; then
    echo "=== Council Health Check ($(date '+%Y-%m-%d %H:%M:%S')) ==="
    for line in "${STATUS_LINES[@]}"; do
        echo "  ${line}"
    done
    echo "==="
    if [[ "${DEGRADED}" -gt 0 ]]; then
        echo "  DEGRADED: ${DEGRADED} service(s) down"
    else
        echo "  ALL HEALTHY"
    fi
fi

# Alert via ming-qiao if degraded
if [[ "${DEGRADED}" -gt 0 ]] && [[ "${ALERT}" == true ]]; then
    DOWN_LIST=""
    for line in "${STATUS_LINES[@]}"; do
        if [[ "${line}" == DOWN* ]]; then
            DOWN_LIST="${DOWN_LIST}\n- ${line#DOWN  }"
        fi
    done

    curl -s -X POST http://localhost:7777/api/threads \
        -H "Content-Type: application/json" \
        -d "$(cat <<EOF
{
    "from": "health-check",
    "to": "council",
    "subject": "ALERT: ${DEGRADED} service(s) down",
    "content": "Health check detected ${DEGRADED} degraded service(s) at $(date '+%Y-%m-%d %H:%M:%S'):\n${DOWN_LIST}\n\nRun health-check.sh for full status.",
    "intent": "request"
}
EOF
)" > /dev/null 2>&1 || true
fi

if [[ "${DEGRADED}" -gt 0 ]]; then
    exit 1
else
    exit 0
fi
