#!/bin/bash
# jikimi-healthd.sh — Jikimi Phase 1 Health Check Daemon
# Council's eighth member: infrastructure heartbeat monitor
#
# Tier 1 (30s):  Critical path — ming-qiao HTTP, NATS, SurrealDB
# Tier 2 (5m):   Important — Ollama, FalkorDB, Graphiti, awakener, notification watcher
# Tier 3 (15m):  Drift — FalkorDB timeout, model pin integrity, ASTROLABE config staleness
#
# Output: Structured JSONL to HEALTH_DIR/YYYY-MM-DD.jsonl
# Alerts: ming-qiao messages (intent:inform) with priority escalation
# Fallback: PENDING_MESSAGES.md when ming-qiao is unreachable

set -uo pipefail

# ── Configuration ──

JIKIMI_HOME="/Users/proteus/astralmaris/ming-qiao/jikimi"
HEALTH_DIR="${JIKIMI_HOME}/health"
HEARTBEAT_FILE="${HEALTH_DIR}/.heartbeat"
PID_FILE="${HEALTH_DIR}/.jikimi-healthd.pid"

MQ_URL="http://localhost:7777"
MQ_TOKEN="mq-jikimi-13ebd293892ef79ba69cad74858a7297"
NATS_URL="localhost:4222"
SURREAL_URL="http://localhost:8000"
OLLAMA_URL="http://localhost:11434"
FALKORDB_CONTAINER="docker-falkordb-1"
GRAPHITI_URL="http://localhost:8001"

MODEL_PIN_FILE="${JIKIMI_HOME}/config/model-pin.toml"
CAPABILITIES_FILE="/Users/proteus/astralmaris/ming-qiao/main/config/agent-capabilities.toml"
NOTIFY_DIR="/Users/proteus/astralmaris/ming-qiao/notifications"

# Intervals in seconds
TIER1_INTERVAL=30
TIER2_INTERVAL=300
TIER3_INTERVAL=900

# Tracking: last run timestamps (epoch seconds)
LAST_TIER1=0
LAST_TIER2=0
LAST_TIER3=0

# Alert state directory (bash 3.2 compat — no associative arrays on macOS)
STATE_DIR="${HEALTH_DIR}/.state"

get_state() { cat "${STATE_DIR}/$1" 2>/dev/null || echo "$2"; }
set_state() { echo "$2" > "${STATE_DIR}/$1"; }

# ── Logging ──

log_health() {
    local check_name="$1"
    local tier="$2"
    local status="$3"  # ok | warn | crit
    local message="$4"
    local today
    today=$(date +%Y-%m-%d)
    local ts
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)

    local jsonl_file="${HEALTH_DIR}/${today}.jsonl"
    printf '{"ts":"%s","check":"%s","tier":%d,"status":"%s","message":"%s","agent":"jikimi"}\n' \
        "$ts" "$check_name" "$tier" "$status" "$message" >> "$jsonl_file"
}

# ── Alert Routing ──

send_alert() {
    local check_name="$1"
    local severity="$2"  # normal | high | critical
    local message="$3"

    # Try ming-qiao first
    local response
    response=$(curl -s --connect-timeout 3 --max-time 5 \
        -X POST "${MQ_URL}/api/threads" \
        -H "Content-Type: application/json" \
        -H "Authorization: Bearer ${MQ_TOKEN}" \
        -d "$(printf '{"from":"jikimi","to":"aleph","subject":"HEALTH ALERT: %s","content":"%s","intent":"inform","priority":"%s","synthetic":true}' \
            "$check_name" "$message" "$severity")" 2>/dev/null)

    if echo "$response" | jq -e '.message_id' >/dev/null 2>&1; then
        return 0
    fi

    # Fallback: PENDING_MESSAGES.md to Aleph's worktree
    fallback_alert "$check_name" "$severity" "$message"
}

fallback_alert() {
    local check_name="$1"
    local severity="$2"
    local message="$3"
    local ts
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)

    local pending_file="/Users/proteus/astralmaris/ming-qiao/aleph/PENDING_MESSAGES.md"
    {
        echo ""
        echo "## HEALTH ALERT — ${check_name} [${severity}]"
        echo "**From:** jikimi (health daemon)"
        echo "**Time:** ${ts}"
        echo "**Status:** ${severity}"
        echo ""
        echo "${message}"
        echo ""
        echo "---"
    } >> "$pending_file"
}

# Track failures and alert on threshold
record_check() {
    local check_name="$1"
    local tier="$2"
    local status="$3"
    local message="$4"

    log_health "$check_name" "$tier" "$status" "$message"

    if [[ "$status" == "ok" ]]; then
        # Recovery: if we previously alerted, send recovery notice
        if [[ "$(get_state "alert_${check_name}" 0)" == "1" ]]; then
            send_alert "$check_name" "normal" "RECOVERED: ${check_name} is healthy again. ${message}"
            set_state "alert_${check_name}" 0
        fi
        set_state "fail_${check_name}" 0
        return
    fi

    local count
    count=$(get_state "fail_${check_name}" 0)
    count=$((count + 1))
    set_state "fail_${check_name}" "$count"

    # Alert thresholds: Tier 1 after 2 consecutive failures (1 min), Tier 2/3 after 1
    local threshold=1
    [[ "$tier" -eq 1 ]] && threshold=2

    if [[ "$count" -ge "$threshold" && "$(get_state "alert_${check_name}" 0)" != "1" ]]; then
        local severity="normal"
        [[ "$status" == "crit" ]] && severity="high"
        [[ "$tier" -eq 1 && "$status" == "crit" ]] && severity="critical"

        send_alert "$check_name" "$severity" "${message} (failed ${count}x)"
        set_state "alert_${check_name}" 1
    fi
}

# ── Tier 1 Checks (30s) — Critical Path ──

check_mingqiao_http() {
    local response
    response=$(curl -s --connect-timeout 3 --max-time 5 "${MQ_URL}/api/threads" 2>/dev/null)
    if echo "$response" | jq -e '.threads' >/dev/null 2>&1; then
        record_check "mingqiao_http" 1 "ok" "HTTP API responding"
    else
        record_check "mingqiao_http" 1 "crit" "ming-qiao HTTP API unreachable or invalid response"
    fi
}

check_nats() {
    # TCP connect test — NATS monitoring port not enabled
    local nats_response
    nats_response=$(echo "" | nc -w 2 localhost 4222 2>&1 || true)
    if echo "$nats_response" | grep -q "INFO"; then
        record_check "nats" 1 "ok" "NATS server accepting connections on 4222"
    else
        record_check "nats" 1 "crit" "NATS server unreachable on port 4222"
    fi
}

check_surrealdb() {
    local response
    response=$(curl -s --connect-timeout 3 --max-time 5 "${SURREAL_URL}/health" 2>/dev/null)
    if [[ "$response" == *"OK"* ]] || [[ "$(curl -s -o /dev/null -w '%{http_code}' --connect-timeout 3 "${SURREAL_URL}/health" 2>/dev/null)" == "200" ]]; then
        record_check "surrealdb" 1 "ok" "SurrealDB health endpoint OK"
    else
        record_check "surrealdb" 1 "crit" "SurrealDB unreachable at ${SURREAL_URL}"
    fi
}

# ── Tier 2 Checks (5m) — Important Services ──

check_ollama() {
    local response
    response=$(curl -s --connect-timeout 3 --max-time 5 "${OLLAMA_URL}/api/tags" 2>/dev/null)
    if echo "$response" | jq -e '.models' >/dev/null 2>&1; then
        local model_count
        model_count=$(echo "$response" | jq '.models | length')
        record_check "ollama" 2 "ok" "Ollama serving ${model_count} models"
    else
        record_check "ollama" 2 "crit" "Ollama API unreachable"
    fi
}

check_falkordb() {
    local result
    result=$(docker exec "$FALKORDB_CONTAINER" redis-cli PING 2>/dev/null)
    if [[ "$result" == "PONG" ]]; then
        record_check "falkordb" 2 "ok" "FalkorDB responding to PING"
    else
        record_check "falkordb" 2 "crit" "FalkorDB container not responding"
    fi
}

check_graphiti() {
    local status_code
    status_code=$(curl -s -o /dev/null -w '%{http_code}' --connect-timeout 3 --max-time 5 "${GRAPHITI_URL}/status" 2>/dev/null)
    if [[ "$status_code" == "200" ]]; then
        record_check "graphiti_mcp" 2 "ok" "Graphiti MCP server responding"
    else
        record_check "graphiti_mcp" 2 "warn" "Graphiti MCP not responding (HTTP ${status_code})"
    fi
}

check_awakener() {
    if launchctl list com.astralmaris.council-awakener >/dev/null 2>&1; then
        local pid
        pid=$(launchctl list com.astralmaris.council-awakener 2>/dev/null | awk 'NR==1{print $1}')
        if [[ "$pid" =~ ^[0-9]+$ ]]; then
            record_check "awakener" 2 "ok" "Council awakener running (PID ${pid})"
        else
            record_check "awakener" 2 "warn" "Awakener loaded but no PID — may have exited"
        fi
    else
        record_check "awakener" 2 "crit" "Council awakener not loaded in launchd"
    fi
}

check_notification_files() {
    local missing=""
    for agent in aleph luban ogma thales mataya laozi-jung merlin jikimi; do
        if [[ ! -f "${NOTIFY_DIR}/${agent}.jsonl" ]]; then
            missing="${missing} ${agent}"
        fi
    done
    if [[ -z "$missing" ]]; then
        record_check "notification_files" 2 "ok" "All agent notification files present"
    else
        record_check "notification_files" 2 "warn" "Missing notification files:${missing}"
    fi
}

# ── Tier 3 Checks (15m) — Drift Detection ──

check_falkordb_timeout() {
    local timeout
    timeout=$(docker exec "$FALKORDB_CONTAINER" redis-cli GRAPH.CONFIG GET TIMEOUT 2>/dev/null | tail -1)
    if [[ -z "$timeout" ]]; then
        record_check "falkordb_timeout" 3 "warn" "Cannot read FalkorDB TIMEOUT config"
        return
    fi
    if [[ "$timeout" -lt 10000 ]]; then
        record_check "falkordb_timeout" 3 "crit" "FalkorDB TIMEOUT is ${timeout}ms — too low (need >=10000). Run: docker exec ${FALKORDB_CONTAINER} redis-cli GRAPH.CONFIG SET TIMEOUT 30000"
    else
        record_check "falkordb_timeout" 3 "ok" "FalkorDB TIMEOUT is ${timeout}ms"
    fi
}

check_model_pins() {
    if [[ ! -f "$MODEL_PIN_FILE" ]]; then
        record_check "model_pins" 3 "warn" "Model pin file not found at ${MODEL_PIN_FILE}"
        return
    fi

    local ollama_models
    ollama_models=$(curl -s --connect-timeout 3 "${OLLAMA_URL}/api/tags" 2>/dev/null)
    if ! echo "$ollama_models" | jq -e '.models' >/dev/null 2>&1; then
        record_check "model_pins" 3 "warn" "Cannot verify model pins — Ollama unreachable"
        return
    fi

    local issues=""

    # Check qwen3:8b
    local expected_8b
    expected_8b=$(awk '/\[models\.default\]/,/digest/' "$MODEL_PIN_FILE" | grep 'digest' | sed 's/.*= *"//;s/".*//')
    # Normalize: strip sha256: prefix for comparison
    expected_8b="${expected_8b#sha256:}"
    if [[ -n "$expected_8b" ]]; then
        local actual_8b
        actual_8b=$(echo "$ollama_models" | jq -r '.models[] | select(.name=="qwen3:8b") | .digest' 2>/dev/null)
        actual_8b="${actual_8b#sha256:}"
        if [[ -n "$actual_8b" && "$actual_8b" != "$expected_8b" ]]; then
            issues="${issues} qwen3:8b digest mismatch (expected ${expected_8b:0:16}... got ${actual_8b:0:16}...)"
        fi
    fi

    # Check qwen3:14b
    local expected_14b
    expected_14b=$(awk '/\[models\.escalation\]/,/digest/' "$MODEL_PIN_FILE" | grep 'digest' | sed 's/.*= *"//;s/".*//')
    expected_14b="${expected_14b#sha256:}"
    if [[ -n "$expected_14b" ]]; then
        local actual_14b
        actual_14b=$(echo "$ollama_models" | jq -r '.models[] | select(.name=="qwen3:14b") | .digest' 2>/dev/null)
        actual_14b="${actual_14b#sha256:}"
        if [[ -n "$actual_14b" && "$actual_14b" != "$expected_14b" ]]; then
            issues="${issues} qwen3:14b digest mismatch (expected ${expected_14b:0:16}... got ${actual_14b:0:16}...)"
        fi
    fi

    if [[ -z "$issues" ]]; then
        record_check "model_pins" 3 "ok" "Model digests match pinned values"
    else
        record_check "model_pins" 3 "crit" "Model pin violation:${issues}"
    fi
}

check_astrolabe_config() {
    local config_file="/Users/proteus/astralmaris/oracle/graphiti/mcp_server/config/config-oracle.yaml"
    if [[ ! -f "$config_file" ]]; then
        record_check "astrolabe_config" 3 "warn" "ASTROLABE config not found"
        return
    fi

    # Check if config references expected group_id
    if grep -q "astrolabe_main" "$config_file" 2>/dev/null || grep -q "group_id" "$config_file" 2>/dev/null; then
        local age_seconds
        age_seconds=$(( $(date +%s) - $(stat -f %m "$config_file") ))
        local age_days=$(( age_seconds / 86400 ))
        if [[ "$age_days" -gt 30 ]]; then
            record_check "astrolabe_config" 3 "warn" "ASTROLABE config is ${age_days} days old — review for staleness"
        else
            record_check "astrolabe_config" 3 "ok" "ASTROLABE config present (${age_days}d old)"
        fi
    else
        record_check "astrolabe_config" 3 "warn" "ASTROLABE config may be stale — missing expected group_id"
    fi
}

check_agent_resolve() {
    # Verify resolve_agent_id works for all known agents
    if [[ ! -f "$CAPABILITIES_FILE" ]]; then
        record_check "agent_resolve" 3 "crit" "agent-capabilities.toml missing"
        return
    fi

    local missing=""
    for agent in aleph luban ogma thales mataya laozi-jung merlin jikimi; do
        if ! grep -q "\\[agents\\.${agent}\\]" "$CAPABILITIES_FILE" 2>/dev/null; then
            missing="${missing} ${agent}"
        fi
    done

    if [[ -z "$missing" ]]; then
        record_check "agent_resolve" 3 "ok" "All agents present in capabilities config"
    else
        record_check "agent_resolve" 3 "crit" "Missing from agent-capabilities.toml:${missing}"
    fi
}

check_nats_permissions() {
    # Verify NATS JetStream consumers exist for agents
    local response
    response=$(curl -s --connect-timeout 3 "http://localhost:8222/jsz" 2>/dev/null)
    if echo "$response" | jq -e '.streams' >/dev/null 2>&1; then
        record_check "nats_jetstream" 3 "ok" "NATS JetStream operational"
    else
        record_check "nats_jetstream" 3 "warn" "Cannot verify NATS JetStream status"
    fi
}

# ── Main Loop ──

update_heartbeat() {
    date -u +%Y-%m-%dT%H:%M:%SZ > "$HEARTBEAT_FILE"
}

run_tier1() {
    check_mingqiao_http
    check_nats
    check_surrealdb
}

run_tier2() {
    check_ollama
    check_falkordb
    check_graphiti
    check_awakener
    check_notification_files
}

run_tier3() {
    check_falkordb_timeout
    check_model_pins
    check_astrolabe_config
    check_agent_resolve
    check_nats_permissions
}

cleanup() {
    rm -f "$PID_FILE"
    log_health "daemon" 0 "ok" "jikimi-healthd shutting down"
    exit 0
}

trap cleanup SIGTERM SIGINT

main() {
    mkdir -p "$HEALTH_DIR" "$STATE_DIR"

    # Write PID
    echo $$ > "$PID_FILE"

    log_health "daemon" 0 "ok" "jikimi-healthd starting (PID $$)"

    # Run all tiers immediately on startup
    run_tier1
    run_tier2
    run_tier3
    LAST_TIER1=$(date +%s)
    LAST_TIER2=$(date +%s)
    LAST_TIER3=$(date +%s)
    update_heartbeat

    while true; do
        sleep 10  # Wake every 10s, check which tiers are due

        local now
        now=$(date +%s)

        if (( now - LAST_TIER1 >= TIER1_INTERVAL )); then
            run_tier1
            LAST_TIER1=$now
            update_heartbeat
        fi

        if (( now - LAST_TIER2 >= TIER2_INTERVAL )); then
            run_tier2
            LAST_TIER2=$now
        fi

        if (( now - LAST_TIER3 >= TIER3_INTERVAL )); then
            run_tier3
            LAST_TIER3=$now
        fi
    done
}

main "$@"
