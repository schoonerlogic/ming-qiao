# Recovery Runbook — Post-Outage Checklist

**Open this first after any system outage, reboot, or service crash.**

Authored from the 2026-03-07 reboot incident. All steps are verified.

---

## Phase 1: Service Health (do first, before starting any agents)

### 1.1 Check all launchd services

```bash
launchctl list | grep astralmaris
```

Expected services (all should show exit code `0`):

| Service | What it does |
|---------|-------------|
| `com.astralmaris.surrealdb` | Event store (ming-qiao persistence) |
| `com.astralmaris.nats-server` | Message bus |
| `com.astralmaris.ming-qiao` | Agent communication HTTP + MCP |
| `com.astralmaris.council-awakener` | Agent wake-on-message |
| `com.astralmaris.council-dispatch` | Event dispatch |
| `com.astralmaris.spire-agent` | SPIFFE identity |
| `com.astralmaris.spire-server` | SPIFFE CA |

**Known issue:** ming-qiao depends on SurrealDB. If SurrealDB starts slowly after reboot, ming-qiao will panic with `WebSocket error: Connection refused (os error 61)` and show exit code `101`. Fix: restart ming-qiao after confirming SurrealDB is up.

### 1.2 Restart any failed services

```bash
# Restart a specific service
launchctl kickstart -k gui/$(id -u)/com.astralmaris.ming-qiao

# Verify health
curl -s http://localhost:7777/health
```

Expected: `{"nats_connected":true,"service":"ming-qiao","status":"healthy","version":"0.1.0"}`

### 1.3 Check Docker containers (ORACLE stack)

```bash
docker ps --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"
```

Expected:
- `docker-graphiti-mcp-1` — healthy, port 8001
- `docker-falkordb-1` — healthy, ports 3000, 6379

If down:
```bash
cd /Users/proteus/astralmaris/oracle/graphiti/mcp_server/docker
docker compose -f docker-compose-oracle.yml up -d
```

### 1.4 FalkorDB timeout (verify after container restart)

The 30s query timeout is now set via `REDIS_ARGS` in docker-compose-oracle.yml and persists across restarts. Verify after any FalkorDB restart:

```bash
docker exec docker-falkordb-1 redis-cli -p 6379 CONFIG GET timeout
# Expected: timeout 30
```

If it shows `0`, the compose file may have been overwritten. Manual fix:
```bash
docker exec docker-falkordb-1 redis-cli -p 6379 CONFIG SET TIMEOUT 30
```

---

## Phase 2: Message Integrity (do before waking agents)

### 2.1 Check for message gaps

Messages sent while ming-qiao was down land in notification files (via awakener) but NOT in SurrealDB. Agents querying via MCP `check_messages` won't see them.

**Detect gaps** — compare notification file line counts vs DB inbox:

```bash
for agent in aleph thales luban mataya ogma laozi-jung; do
  file_today=$(grep -c "$(date +%Y-%m-%d)" /Users/proteus/astralmaris/ming-qiao/notifications/${agent}.jsonl 2>/dev/null || echo 0)
  db_today=$(curl -s "http://localhost:7777/api/inbox/${agent}?limit=500" | python3 -c "
import sys, json
data = json.load(sys.stdin)
print(len([m for m in data.get('messages',[]) if '$(date +%Y-%m-%d)' in m.get('timestamp','')]))
" 2>/dev/null || echo 0)
  gap=$((file_today - db_today))
  echo "${agent}: file=${file_today} db=${db_today} gap=${gap}"
done
```

### 2.2 Re-inject missing messages

If gaps exist, re-inject from notification files into SurrealDB via MCP:

```python
import json, subprocess

MQ_BIN = '/Users/proteus/astralmaris/ming-qiao/main/target/release/ming-qiao'
NOTIFICATIONS = '/Users/proteus/astralmaris/ming-qiao/notifications'

def reinject(agent, line_indices):
    """Re-inject notification file messages into SurrealDB."""
    with open(f'{NOTIFICATIONS}/{agent}.jsonl') as f:
        lines = f.readlines()

    for idx in line_indices:
        msg = json.loads(lines[idx].strip())
        rpc = {
            'jsonrpc': '2.0', 'id': 1,
            'method': 'tools/call',
            'params': {
                'name': 'send_message',
                'arguments': {
                    'to': agent,
                    'subject': msg.get('subject', ''),
                    'content': msg.get('content', msg.get('body', '')),
                    'intent': msg.get('intent', 'inform'),
                    'priority': msg.get('priority', 'normal')
                }
            }
        }
        env = {
            'MING_QIAO_AGENT_ID': msg.get('from', 'thales'),
            'MING_QIAO_CONFIG': '/Users/proteus/astralmaris/ming-qiao/aleph/ming-qiao.toml',
            'MINGQIAO_DB_USERNAME': 'mingqiao_service',
            'MINGQIAO_DB_PASSWORD': '<from env or 1Password>',
            'PATH': '/usr/local/bin:/usr/bin:/bin'
        }
        proc = subprocess.run(
            [MQ_BIN, 'mcp-serve'],
            input=json.dumps(rpc),
            capture_output=True, text=True, timeout=10, env=env
        )
        ok = 'successfully' in proc.stdout
        print(f"{'✓' if ok else '✗'} {msg.get('from')} → {agent}: {msg.get('subject','')[:50]}")
```

### 2.3 Rehydrate the in-memory indexer

The HTTP server's in-memory indexer doesn't see events written by MCP subprocesses or direct DB writes. Use the rehydrate endpoint to re-sync without restarting:

```bash
curl -s -X POST http://localhost:7777/api/admin/rehydrate \
  -H "Authorization: Bearer <any-valid-agent-token>" \
  -H "Content-Type: application/json"
```

Expected: `{"status":"rehydrated","events_processed":N,"events_skipped":0,...}`

If the rehydrate endpoint is unavailable (older binary), fall back to a full restart:
```bash
launchctl kickstart -k gui/$(id -u)/com.astralmaris.ming-qiao
sleep 3
curl -s http://localhost:7777/health
```

Check hydration count in logs:
```bash
grep "hydrated" /Users/proteus/astralmaris/ming-qiao/main/logs/ming-qiao.log | tail -1
```

### 2.4 Set lastread baselines for agents without one

Agents without a `.lastread` file will see their entire message history on first check. Set baseline to just before today's messages:

```bash
for agent in mataya ogma laozi-jung luban; do
  lastread="/Users/proteus/astralmaris/ming-qiao/notifications/${agent}.lastread"
  if [ ! -f "$lastread" ]; then
    total=$(wc -l < "/Users/proteus/astralmaris/ming-qiao/notifications/${agent}.jsonl")
    today=$(grep -c "$(date +%Y-%m-%d)" "/Users/proteus/astralmaris/ming-qiao/notifications/${agent}.jsonl")
    baseline=$((total - today))
    echo "$baseline" > "$lastread"
    echo "${agent}: lastread set to ${baseline} (${today} unread)"
  fi
done
```

---

## Phase 3: Verify before waking agents

### 3.1 Inbox spot-check

Pick any agent and verify the inbox returns today's messages sorted by recency:

```bash
curl -s "http://localhost:7777/api/inbox/thales?limit=5" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for m in data.get('messages', []):
    print(f'{m[\"timestamp\"][:19]} | {m[\"from\"]:10} | {m[\"subject\"][:50]}')
"
```

### 3.2 MCP tool check

Verify MCP tools work for at least one agent:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | \
  MING_QIAO_AGENT_ID=aleph \
  MING_QIAO_CONFIG=/Users/proteus/astralmaris/ming-qiao/aleph/ming-qiao.toml \
  MINGQIAO_DB_USERNAME=mingqiao_service \
  MINGQIAO_DB_PASSWORD='<password>' \
  /Users/proteus/astralmaris/ming-qiao/main/target/release/ming-qiao mcp-serve 2>/dev/null | \
  python3 -c "import sys,json; print(len(json.loads(sys.stdin.readline())['result']['tools']), 'tools')"
```

Expected: `12 tools`

### 3.3 ORACLE check

```bash
curl -s http://localhost:8001/mcp -X POST \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | \
  python3 -c "import sys,json; r=json.loads(sys.stdin.read()); print(len(r.get('result',{}).get('tools',[])), 'tools')"
```

---

## Phase 4: Wake agents

### 4.1 Pre-wake: verify MCP configs exist

Before waking any agent, confirm their MCP config is in place. An agent without MCP config will appear to wake but `check_messages` will silently return nothing.

```bash
# Claude Code agents
ls /Users/proteus/astralmaris/astral-forge/aleph/.mcp.json          # Aleph
ls /Users/proteus/astralmaris/inference-kitchen/luban/.mcp.json      # Luban

# Kimi CLI agent (global config)
kimi mcp list                                                        # Mataya

# Check others as needed
ls /Users/proteus/astralmaris/echoessence/main/.mcp.json             # Laozi-Jung (if exists)
```

If missing, create before starting the agent — not after. See `Agent MCP Configs` section in Aleph's MEMORY.md for templates.

### 4.2 Start agents in this order

1. **Thales** — architecture, writes morning brief
2. **Aleph** — infrastructure, verifies services
3. **Ogma** — security, can run independently
4. **Luban** — inference, reads brief
5. **Mataya** — design, reads brief
6. **Laozi-Jung** — witness, ingests everything

**Note:** Ogma's wake path is not configured in the awakener. He must be started manually.

---

## Known Issues & Workarounds

### Ming-qiao startup race condition
**Symptom:** Exit code 101, `WebSocket error: Connection refused`
**Cause:** SurrealDB not ready when ming-qiao starts
**Fix:** Restart ming-qiao after SurrealDB is up
**Proper fix needed:** Add retry/backoff to SurrealDB connection in ming-qiao startup

### Kimi CLI (Mataya) MCP tool results
**Symptom:** `check_messages` returns stale or unsorted results in Kimi sessions
**Workaround:** Use curl to HTTP API as fallback (documented in Mataya's AGENT.md)
**Root cause:** Under investigation — MCP server works correctly when tested standalone

### NATS presence permissions
**Symptom:** Repeated `Permissions Violation for Publish to "am.agent.http-server.presence"` in logs
**Impact:** Cosmetic — does not affect message delivery
**Fix needed:** NATS permission config for presence subjects

### Agent ID mismatch for worktree agents
**Symptom:** Messages sent as `from: main` instead of the actual agent name
**Cause:** Agents whose worktree directory is named `main` (e.g., Laozi-Jung at `echoessence/main`) auto-detect as agent ID `main` in legacy mq-send.sh/mq-inbox.sh scripts
**Fix:** Hardcode agent ID in scripts — do not rely on directory name detection
**Impact:** Messages delivered but unattributable — breaks inbox filtering by sender

### FalkorDB timeout persistence — RESOLVED
**Was:** 30s timeout reset to 0 on every container restart
**Fix:** Added `--timeout 30` to `REDIS_ARGS` in docker-compose-oracle.yml (2026-03-07)

### Indexer drift (messages exist in DB but not in inbox API)
**Symptom:** Messages visible via direct SurrealDB query but absent from `/api/inbox` or `check_messages`
**Cause:** Events written to SurrealDB via MCP subprocess (Path B) bypass the in-memory indexer
**Fix:** `POST /api/admin/rehydrate` — rebuilds indexer from SurrealDB without restart
**Permanent fix:** Eliminate Path B — all writes must go through HTTP API (see Thales decision 2026-03-07)

### Message delivery during outage
**Symptom:** Messages land in notification files but not SurrealDB
**Cause:** Awakener writes to files directly; SurrealDB writes require running ming-qiao HTTP server
**Fix needed:** Awakener should queue failed deliveries for retry, or write directly to SurrealDB

---

## Quick Reference

| Service | Port | Health Check |
|---------|------|-------------|
| ming-qiao HTTP | 7777 | `curl localhost:7777/health` |
| SurrealDB | 8000 | `curl localhost:8000/health` |
| NATS | 4222 | `nats server check connection` |
| FalkorDB | 6379 | `redis-cli -p 6379 ping` |
| Graphiti MCP | 8001 | `curl localhost:8001/mcp` (POST tools/list) |
| SPIRE | — | `launchctl list \| grep spire` |

| File | Purpose |
|------|---------|
| `/Users/proteus/astralmaris/ming-qiao/notifications/{agent}.jsonl` | Notification file per agent |
| `/Users/proteus/astralmaris/ming-qiao/notifications/{agent}.lastread` | Last read line number |
| `/Users/proteus/astralmaris/ming-qiao/main/logs/ming-qiao.log` | stdout log |
| `/Users/proteus/astralmaris/ming-qiao/main/logs/ming-qiao.err` | stderr/panic log |
