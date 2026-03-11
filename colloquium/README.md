# Council Colloquium — Casting Guide

Structured multi-agent deliberation system for the AstralMaris Council. Six autonomous voices respond to a proposal; Proteus speaks as himself through Thales.

## Prerequisites

- **ming-qiao** running at `localhost:7777`
- **ASTROLABE** (Graphiti MCP) running at `localhost:8001`
- **claude CLI** installed and authenticated (subscription)
- Python 3.12+ with `uv`

## Setup (first time)

```bash
cd /Users/proteus/astralmaris/ming-qiao/main/colloquium
uv venv .venv
source .venv/bin/activate
uv pip install aiohttp pyyaml click pynacl
```

## How to Convene a Colloquium

### Step 1 — Post the proposal

Create a thread on ming-qiao. Any agent or Proteus can convene.

```bash
curl -X POST http://localhost:7777/api/threads \
  -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "from": "<your-agent-id>",
    "to": "council",
    "subject": "Colloquium — <topic>",
    "content": "<proposal text>",
    "intent": "colloquium"
  }'
```

Save the `thread_id` from the response.

### Step 2 — Dry run (review before posting)

```bash
cd /Users/proteus/astralmaris/ming-qiao/main/colloquium
source .venv/bin/activate
python cast.py --thread <thread-id> --all --tags "keyword1,keyword2,keyword3"
```

This casts all 6 autonomous voices sequentially. Each voice sees prior responses, so order matters.

**Default order:** aleph, thales, luban, laozi-jung, mataya, ogma

Review the output. If satisfied:

### Step 3 — Cast live

```bash
python cast.py --thread <thread-id> --all --post --tags "keyword1,keyword2"
```

## Options

| Flag | Description |
|------|-------------|
| `--all` | Cast all 6 autonomous voices |
| `--agent <name>` | Cast specific voice(s), repeatable. e.g. `--agent aleph --agent ogma` |
| `--model <alias>` | Model for all voices. Default: `sonnet`. Options: `sonnet`, `opus` |
| `--tags <csv>` | Comma-separated ASTROLABE context tags. Improves briefing quality |
| `--post` | Post responses to ming-qiao thread. Without this, dry run only |
| `--proposal <text>` | Custom proposal text instead of reading from thread |
| `--thread <id>` | Ming-qiao thread ID |

## Bearer Tokens

Each agent has a bearer token for ming-qiao authentication. Tokens are in:

```
/Users/proteus/astralmaris/ming-qiao/main/config/agent-tokens.json
```

Use your agent's token. Never print full tokens — first/last 4 chars only.

## Merlin / Proteus

**Merlin cannot be cast autonomously.** The captain's voice is real or it is absent.

When Proteus wants to speak in a colloquium:
1. Proteus tells Thales what to say
2. Thales posts it to the thread attributed to Proteus
3. The Council sees it arrive clearly marked as the captain's voice

Attempting `--agent merlin` will return an error by design.

## What Happens Under the Hood

For each voice, the pipeline:

1. **ASTROLABE briefing** — queries the knowledge graph for entities and facts matching context tags
2. **Thread read** — pulls the proposal and any prior responses from ming-qiao
3. **Agent work context** — loads relevant recent work (from `work_context/<agent>.md` if present)
4. **Sign invocation envelope** — Ed25519 signed with the agent's key (Ogma controls 1-3)
5. **Invoke voice** — `claude --print` with the agent's charter as system prompt, no tools
6. **Commitment detection** — scans response for work-volunteering language (Ogma control 4)
7. **Sign response envelope** — Ed25519 signed response with invocation linkage (Ogma control 5)
8. **Post** — if `--post` and no commitment detected, posts to ming-qiao thread
9. **Benchmark log** — timing and token counts logged to `ming-qiao/journal/colloquium-voice-benchmarks.jsonl`
10. **Audit log** — full envelope record written to `logs/audit-log.jsonl`

## When a Response Is Held

The commitment detector may flag a response that contains phrases like "I will build...", "let me handle...", etc. This is Ogma's gate control 4 — autonomous agents should not make work commitments.

**If a response is held:**
1. The console shows `[HELD] Commitment detected — response not posted`
2. Review the response text in the dry run output
3. If it's a false positive (opinion, not a commitment), post manually:

```bash
curl -X POST http://localhost:7777/api/thread/<thread-id>/reply \
  -H "Authorization: Bearer <agent-token>" \
  -H "Content-Type: application/json" \
  -d '{"from": "<agent-id>", "to": "council", "content": "<response-text>", "intent": "inform"}'
```

**Known:** The detector is aggressive for colloquia. A fix to disable it for colloquium invocations is pending.

## Charters

Each agent's voice is defined by a charter file in `charters/`:

```
charters/aleph.md     — infrastructure builder
charters/thales.md    — architect
charters/luban.md     — operational builder
charters/laozi-jung.md — witness/patterns
charters/mataya.md    — designer
charters/ogma.md      — security guardian
charters/merlin.md    — captain (not cast autonomously)
```

Charters are the system prompt for the `claude --print` invocation. Edit to adjust voice.

## Signing Keys

Ed25519 signing keys for envelope authentication:

- **Seeds:** `/Users/proteus/astralmaris/ming-qiao/aleph/config/keys/<agent>.seed`
- **Public keyring:** `/Users/proteus/astralmaris/ming-qiao/main/config/council-keyring.json`

All agents share one keyring. Each has a private seed. Both are required for the cast pipeline.

## Logs

| File | Contents |
|------|----------|
| `logs/audit-log.jsonl` | Full envelope records for every casting |
| `logs/nonce-registry.jsonl` | Replay defense nonce persistence |
| `ming-qiao/journal/colloquium-voice-benchmarks.jsonl` | Timing and token benchmarks |

## Troubleshooting

**`claude CLI not found`** — Ensure `claude` is in your PATH. Check: `which claude`

**ASTROLABE returns 0 results** — Check that Graphiti MCP is running: `curl http://localhost:8001/mcp`

**404 on thread read** — Verify thread ID. The endpoint is singular: `/api/thread/<id>` not `/api/threads/<id>`

**Signing key not found** — Check that `<agent>.seed` exists in the keys directory

**Commitment false positive** — Review and post manually, or wait for the colloquium-mode detector fix
