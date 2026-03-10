# Recovery Runbook — Post-Outage Checklist

**Open this first after any system outage, reboot, or service crash.**

Authored from the 2026-03-07 and 2026-03-09 incidents. All steps are verified.

---

## Quick Decision Tree

```
Machine just rebooted?
  → Phase 1 (services) → Phase 2 (messages) → Phase 3 (verify) → Phase 4 (identity) → Phase 5 (agents)

Changed agent config only (TOML, .mcp.json)?
  → Phase 4.2 (verify the config) → restart the affected service/agent session

Changed NATS auth config?
  → kill -HUP $(pgrep nats-server) → Phase 1.2 (health check) → Phase 4.3 (NATS verify)

Adding a new agent?
  → Create NKey, TOML config, .mcp.json, agent-capabilities entry → Phase 4 (full identity check)
```

---

## Phase 1: Service Health (do first, before starting any agents)

### 1.1 Check all launchd services

```bash
launchctl list | grep astralmaris
```

Expected services (all should show exit code `0`):

| Service | What it does | Depends on |
|---------|-------------|------------|
| `com.astralmaris.surrealdb` | Event store (ming-qiao persistence) | — |
| `com.astralmaris.nats-server` | Message bus | — |
| `com.astralmaris.ming-qiao` | Agent communication HTTP + MCP | SurrealDB, NATS |
| `com.astralmaris.council-awakener` | Agent wake-on-message | NATS |
| `com.astralmaris.council-dispatch` | Event dispatch | ming-qiao |
| `com.astralmaris.spire-agent` | SPIFFE identity | SPIRE server |
| `com.astralmaris.spire-server` | SPIFFE CA | — |

**Start order matters:** SurrealDB and NATS must be up before ming-qiao. launchd starts them all simultaneously on boot, so ming-qiao may fail on first attempt.

**Known issue:** ming-qiao depends on SurrealDB. If SurrealDB starts slowly after reboot, ming-qiao will panic with `WebSocket error: Connection refused (os error 61)` and show exit code `101`. Fix: restart ming-qiao after confirming SurrealDB is up.

### 1.2 Restart any failed services

```bash
# Restart a specific service
launchctl kickstart -k gui/$(id -u)/com.astralmaris.ming-qiao

# Verify health
curl -s http://localhost:7777/health
```

Expected: `{"nats_connected":true,"service":"ming-qiao","status":"healthy","version":"0.1.0"}`

**Critical fields to check:**
- `nats_connected: true` — if false, NATS server may be down or NKey auth failed
- `status: healthy` — if not, check stderr log

### 1.3 Check Docker containers (ORACLE stack)

OrbStack is in macOS Login Items and auto-starts on boot. Docker containers with `restart: unless-stopped` will auto-start.

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
        print(f'{"OK" if ok else "FAIL"} {msg.get("from")} -> {agent}: {msg.get("subject","")[:50]}')
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

## Phase 4: Identity & Configuration Verification

**Lesson from 2026-03-09:** Identity mismatches and NATS config gaps caused messages to be sent under wrong identities and agents to miss broadcasts entirely. Run this phase after every reboot and after any config change.

### 4.1 Agent Configuration Matrix

Every agent needs **four things** to function correctly:

| Agent | Runtime | MCP Config Location | TOML Config | NKey File |
|-------|---------|-------------------|-------------|-----------|
| Aleph | Claude Code | `astral-forge/aleph/.mcp.json` | `ming-qiao/aleph/ming-qiao.toml` | `config/nkeys/aleph.nk` |
| Thales | Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` | `ming-qiao/main/ming-qiao-thales.toml` | `config/nkeys/thales.nk` |
| Luban | Claude Code | `inference-kitchen/luban/.mcp.json` | `ming-qiao/luban/ming-qiao.toml` | `config/nkeys/luban.nk` |
| Mataya | Kimi Code | `latent-winds/mataya/.mcp.json` | `ming-qiao/mataya/ming-qiao.toml` | `config/nkeys/mataya.nk` |
| Ogma | Claude Code | `everwatch-spire/ogma/.mcp.json` | `ming-qiao/main/ming-qiao-ogma.toml` | `config/nkeys/ogma.nk` |
| Laozi-Jung | Kimi Code | `~/.kimi/mcp.json` (global) | `ming-qiao/main/ming-qiao-laozi-jung.toml` | `config/nkeys/laozi-jung.nk` |
| Merlin | Manual | `ming-qiao/merlin/.mcp.json` | `ming-qiao/merlin/ming-qiao.toml` | `config/nkeys/merlin.nk` |
| Council-Chamber | Service | — | `ming-qiao/main/ming-qiao-council-chamber.toml` | `config/nkeys/council-chamber.nk` |
| _server (HTTP) | launchd | — | `ming-qiao/aleph/ming-qiao.toml` | `config/nkeys/aleph.nk` |

All paths relative to `/Users/proteus/astralmaris/` unless absolute.
NKey files relative to `/Users/proteus/astralmaris/ming-qiao/main/`.

### 4.2 Identity Verification Checklist

Run after reboot or config change. Verifies that each agent's MCP config points to the correct identity.

```bash
echo "=== Claude Code agents (.mcp.json) ==="
for path in \
  /Users/proteus/astralmaris/astral-forge/aleph/.mcp.json \
  /Users/proteus/astralmaris/inference-kitchen/luban/.mcp.json \
  /Users/proteus/astralmaris/everwatch-spire/ogma/.mcp.json \
; do
  agent_id=$(python3 -c "import json; print(json.load(open('$path'))['mcpServers']['ming-qiao']['env']['MING_QIAO_AGENT_ID'])" 2>/dev/null || echo "MISSING")
  config=$(python3 -c "import json; print(json.load(open('$path'))['mcpServers']['ming-qiao']['env']['MING_QIAO_CONFIG'])" 2>/dev/null || echo "MISSING")
  echo "  $(basename $(dirname $(dirname $path)))/$(basename $(dirname $path)): agent_id=$agent_id config=$config"
done

echo ""
echo "=== Kimi global config ==="
kimi_id=$(python3 -c "import json; print(json.load(open('/Users/proteus/.kimi/mcp.json'))['mcpServers']['ming-qiao']['env']['MING_QIAO_AGENT_ID'])" 2>/dev/null || echo "MISSING")
echo "  ~/.kimi/mcp.json: agent_id=$kimi_id (should be laozi-jung)"

echo ""
echo "=== Claude Desktop ==="
desktop_id=$(python3 -c "
import json
c = json.load(open('/Users/proteus/Library/Application Support/Claude/claude_desktop_config.json'))
print(c['mcpServers']['ming-qiao']['env']['MING_QIAO_AGENT_ID'])
" 2>/dev/null || echo "MISSING")
echo "  Claude Desktop: agent_id=$desktop_id (should be thales)"
```

**What to look for:**
- Each agent_id must match the agent name exactly
- Config paths must point to the agent-specific TOML (not another agent's)
- DB credentials (`MINGQIAO_DB_USERNAME`, `MINGQIAO_DB_PASSWORD`) must be present

**Known pitfall (2026-03-09):** The Kimi global config at `~/.kimi/mcp.json` applies to ALL Kimi Code sessions. If you change it for one agent, it changes for all Kimi-based agents. Currently only Laozi-Jung uses Kimi Code; Mataya uses a per-project `.mcp.json` in `latent-winds/mataya/`. If Mataya is moved to Kimi Code globally, identity conflicts will recur.

### 4.3 NATS + NKey Verification

Every agent TOML must have NATS enabled with NKey auth. Without this, the agent can still read/write via SurrealDB but will not receive real-time notifications or trigger the awakener.

```bash
echo "=== NATS config in agent TOMLs ==="
for toml in \
  /Users/proteus/astralmaris/ming-qiao/aleph/ming-qiao.toml \
  /Users/proteus/astralmaris/ming-qiao/main/ming-qiao-thales.toml \
  /Users/proteus/astralmaris/ming-qiao/luban/ming-qiao.toml \
  /Users/proteus/astralmaris/ming-qiao/mataya/ming-qiao.toml \
  /Users/proteus/astralmaris/ming-qiao/main/ming-qiao-ogma.toml \
  /Users/proteus/astralmaris/ming-qiao/main/ming-qiao-laozi-jung.toml \
  /Users/proteus/astralmaris/ming-qiao/merlin/ming-qiao.toml \
  /Users/proteus/astralmaris/ming-qiao/main/ming-qiao-council-chamber.toml \
; do
  name=$(basename $toml .toml)
  nats_enabled=$(grep -A1 '^\[nats\]' "$toml" 2>/dev/null | grep 'enabled' | grep -o 'true\|false' || echo "MISSING")
  auth_mode=$(grep 'auth_mode' "$toml" 2>/dev/null | grep -o '"[^"]*"' || echo "MISSING")
  nkey_file=$(grep 'nkey_seed_file' "$toml" 2>/dev/null | grep -o '"[^"]*"' || echo "MISSING")
  echo "  $name: nats=$nats_enabled auth=$auth_mode nkey=$nkey_file"
done
```

**All must show:** `nats=true auth="nkey" nkey="/Users/proteus/astralmaris/ming-qiao/main/config/nkeys/<agent>.nk"`

**Known pitfall (2026-03-09):** NKey seed file paths must be **absolute**. Relative paths (e.g., `config/nkeys/aleph.nk`) break when MCP subprocesses are spawned from different working directories.

### 4.4 Awakener Capabilities Check

The council-awakener reads `agent-capabilities.toml` to know which agents exist and how to wake them. An agent missing from this file will never be woken by incoming messages.

```bash
echo "=== Agents in agent-capabilities.toml ==="
grep '^\[agents\.' /Users/proteus/astralmaris/ming-qiao/main/config/agent-capabilities.toml | \
  sed 's/\[agents\.\(.*\)\]/  \1/'

echo ""
echo "=== Expected agents ==="
echo "  aleph, luban, ogma, mataya, laozi-jung, thales, merlin"
```

If an agent is missing, add it to `agent-capabilities.toml` and restart the awakener:

```bash
launchctl kickstart -k gui/$(id -u)/com.astralmaris.council-awakener
```

---

## Phase 5: Wake Agents

### 5.1 Pre-wake: verify MCP configs exist

Before waking any agent, confirm their MCP config is in place. An agent without MCP config will appear to wake but `check_messages` will silently return nothing.

```bash
echo "=== MCP config existence check ==="
for f in \
  /Users/proteus/astralmaris/astral-forge/aleph/.mcp.json \
  /Users/proteus/astralmaris/inference-kitchen/luban/.mcp.json \
  /Users/proteus/astralmaris/everwatch-spire/ogma/.mcp.json \
  /Users/proteus/astralmaris/latent-winds/mataya/.mcp.json \
  "/Users/proteus/Library/Application Support/Claude/claude_desktop_config.json" \
  /Users/proteus/.kimi/mcp.json \
; do
  if [ -f "$f" ]; then echo "  OK: $f"; else echo "  MISSING: $f"; fi
done
```

### 5.2 Start agents in this order

1. **Thales** (Claude Desktop) — architecture, writes morning brief. Start by opening Claude Desktop app.
2. **Aleph** (Claude Code) — infrastructure, verifies services. Start with `claude` in `astral-forge/aleph/`.
3. **Ogma** (Claude Code) — security. Start with `claude` in `everwatch-spire/ogma/`.
4. **Luban** (Claude Code) — inference. Start with `claude` in `inference-kitchen/luban/`.
5. **Mataya** (Kimi Code) — design. Start with `kimi` in `latent-winds/mataya/`.
6. **Laozi-Jung** (Kimi Code) — witness. Start with `kimi` in `echoessence/main/`.

**Starting a Claude Code agent:**
```bash
cd /Users/proteus/astralmaris/<repo>/<worktree>
claude
```

**Starting Thales (Claude Desktop):**
Open the Claude Desktop app from the Dock or Applications. It reads config from `~/Library/Application Support/Claude/claude_desktop_config.json` automatically. After config changes, you must quit and reopen Claude Desktop.

**Starting a Kimi Code agent:**
```bash
cd /Users/proteus/astralmaris/<repo>/<worktree>
kimi
```

**Note on Kimi Code identity:** Kimi uses `~/.kimi/mcp.json` globally. Currently set to `laozi-jung`. If starting Mataya via Kimi Code, Mataya's per-project `.mcp.json` in `latent-winds/mataya/` should override the global config. Verify by checking the agent's first message in ming-qiao.

### 5.3 Post-wake verification

After starting each agent, verify they can communicate:

```bash
# Check the last message from an agent (should see a recent timestamp)
curl -s "http://localhost:7777/api/inbox/aleph?limit=1" | python3 -c "
import sys, json
data = json.load(sys.stdin)
msgs = data.get('messages', [])
if msgs:
    print(f'Latest: {msgs[0][\"timestamp\"][:19]} from {msgs[0][\"from\"]}: {msgs[0][\"subject\"][:50]}')
else:
    print('No messages yet')
"
```

---

## Phase 6: Configuration Changes (no reboot needed)

### 6.1 Changed an agent's TOML config

The TOML config is read at MCP subprocess startup, not by the long-running HTTP server. Changes take effect when:
- **Claude Code / Kimi Code agents:** Start a new conversation (each conversation spawns a fresh MCP process)
- **Claude Desktop (Thales):** Quit and reopen Claude Desktop
- **HTTP server:** `launchctl kickstart -k gui/$(id -u)/com.astralmaris.ming-qiao`

### 6.2 Changed NATS auth config

NATS auth is at `/Users/proteus/astralmaris/astrallation/configs/nats-auth.conf` (not version-controlled).

```bash
# Reload without restart (graceful)
kill -HUP $(pgrep nats-server)

# Verify
curl -s http://localhost:7777/health | python3 -c "import sys,json; print(json.loads(sys.stdin.read())['nats_connected'])"
# Expected: True
```

After NATS auth changes, agents with active MCP sessions will lose their NATS connection. They need to start a new conversation to reconnect.

### 6.3 Changed agent-capabilities.toml

```bash
# Restart awakener to pick up changes
launchctl kickstart -k gui/$(id -u)/com.astralmaris.council-awakener
```

### 6.4 Changed .mcp.json or Claude Desktop config

- **Claude Code:** Start a new conversation in the same directory
- **Claude Desktop:** Quit and reopen the app
- **Kimi Code:** Start a new conversation

No service restart needed — MCP configs are read per-conversation.

---

## Known Issues & Workarounds

### Ming-qiao startup race condition
**Symptom:** Exit code 101, `WebSocket error: Connection refused`
**Cause:** SurrealDB not ready when ming-qiao starts
**Fix:** Restart ming-qiao after SurrealDB is up
**Proper fix needed:** Add retry/backoff to SurrealDB connection in ming-qiao startup

### Kimi global config identity conflict (2026-03-09)
**Symptom:** Agent sends messages as wrong identity (e.g., Laozi-Jung messages attributed to Mataya)
**Cause:** `~/.kimi/mcp.json` is global — changing it for one Kimi agent changes all Kimi sessions
**Current state:** Set to `laozi-jung`. Mataya uses per-project `.mcp.json` override.
**Fix if recurs:** Check `~/.kimi/mcp.json` MING_QIAO_AGENT_ID matches the intended agent. Long-term fix: Kimi identity wrapper script (deferred to am-fleet).

### Claude Desktop config path confusion (2026-03-09)
**Symptom:** Thales can't access ming-qiao MCP tools
**Cause:** Config pointed to wrong binary path and wrong agent config
**Fix:** Verify `~/Library/Application Support/Claude/claude_desktop_config.json` has correct binary path (`main/target/release/ming-qiao`), correct agent ID (`thales`), correct TOML (`ming-qiao-thales.toml`), and DB credentials.

### NATS disabled on agents (2026-03-09)
**Symptom:** Agent can send/receive via SurrealDB but never triggers awakener, never receives real-time notifications
**Cause:** Agent TOML had `[nats] enabled = false` or missing NKey auth
**Fix:** Enable NATS + NKey in all agent TOMLs (Phase 4.3 catches this)

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
| NATS | 4222 | `nats server check connection --nkey <nkey-file>` |
| FalkorDB | 6379 | `docker exec docker-falkordb-1 redis-cli -p 6379 ping` |
| Graphiti MCP | 8001 | `curl localhost:8001/mcp` (POST tools/list) |
| SPIRE | — | `launchctl list \| grep spire` |

| File | Purpose |
|------|---------|
| `ming-qiao/notifications/{agent}.jsonl` | Notification file per agent |
| `ming-qiao/notifications/{agent}.lastread` | Last read line number |
| `ming-qiao/main/logs/ming-qiao.log` | stdout log |
| `ming-qiao/main/logs/ming-qiao.err` | stderr/panic log |
| `ming-qiao/main/config/agent-capabilities.toml` | Awakener agent roster |
| `ming-qiao/main/config/nkeys/*.nk` | NKey seed files for NATS auth |
| `ming-qiao/main/config/agent-tokens.json` | HTTP API bearer tokens |
| `astrallation/configs/nats-auth.conf` | NATS auth config (not version-controlled) |
| `astrallation/configs/nats-server.conf` | NATS server config |

| Key Command | Purpose |
|-------------|---------|
| `launchctl list \| grep astralmaris` | Check all services |
| `launchctl kickstart -k gui/$(id -u)/com.astralmaris.<svc>` | Restart a service |
| `kill -HUP $(pgrep nats-server)` | Reload NATS config without restart |
| `curl -s localhost:7777/health` | ming-qiao health check |
| `nats sub 'am.events.>' --nkey <nkey>` | Watch live NATS events |
