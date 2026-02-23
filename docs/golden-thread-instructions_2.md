# Ming-Qiao Golden Thread — Persistence Path Verification

**Date:** 2026-02-21
**Issued by:** Thales (Architect) on behalf of Proteus
**Revision:** 2 — updated with actual endpoints from Aleph's implementation

---

## Context

Aleph completed the SurrealDB integration and HTTP write handlers on `agent/aleph/surrealdb-integration`. 123/123 tests pass. The HTTP gateway now supports the full thread lifecycle:

```
POST /api/threads           → create thread + message, persist to SurrealDB, index, broadcast
GET  /api/inbox/{agent}     → read messages from indexer
POST /api/thread/:id/reply  → reply within thread, persist, index, broadcast
```

The persistence path is: **HTTP → SurrealDB → Indexer → HTTP read**. NATS real-time notification will layer on after this verification.

---

## Golden Thread Sequence

```
Luban  → POST /api/threads          → creates thread + message → SurrealDB + Indexer
Aleph  → GET  /api/inbox/aleph      → reads from indexer → sees Luban's message
Aleph  → POST /api/thread/:id/reply → reply persists in same thread
Luban  → GET  /api/inbox/luban      → reads from indexer → sees Aleph's reply
```

Four hops. Round trip confirmed.

---

## Prerequisites — Proteus

### 1. Start SurrealDB

```bash
docker run -d --name surrealdb \
  -p 8000:8000 \
  surrealdb/surrealdb:latest start --log trace --user root --pass root memory
```

### 2. Start NATS JetStream

```bash
docker run -d --name nats \
  -p 4222:4222 \
  -p 8222:8222 \
  nats:latest -js -m 8222
```

### 3. Build and run ming-qiao

```bash
cd ~/astralmaris/ming-qiao/aleph
cargo build
# Set env vars as needed (check .env or src/config)
export NATS_URL="nats://localhost:4222"
export SURREALDB_URL="http://localhost:8000"
export SURREALDB_USER="root"
export SURREALDB_PASS="root"
cargo run serve
```

### 4. Verify

```bash
curl http://localhost:7777/health
```

---

## Task: Luban — Golden Thread (Sender)

### Assignment

Send the first message and confirm receipt of Aleph's reply. **This is a manual test — no code changes.**

### Steps

**Step 1: Confirm gateway is up**

```bash
curl http://localhost:7777/health
```

If this fails, STOP and report to Proteus.

**Step 2: Send the golden thread message**

```bash
curl -s -X POST http://localhost:7777/api/threads \
  -H "Content-Type: application/json" \
  -d '{
    "from": "luban",
    "to": "aleph",
    "subject": "am.agent.council.golden-thread",
    "content": "明桥通了吗？ (Is the bright bridge open?)"
  }' | jq .
```

Record the returned `thread_id`, `message_id`, and `created_at`.

**Step 3: Wait for Aleph's reply, then check inbox**

```bash
curl -s http://localhost:7777/api/inbox/luban | jq .
```

Poll until you see Aleph's reply in your inbox.

**Step 4: Report**

```
GOLDEN THREAD — LUBAN REPORT

Message sent: [yes/no]
  HTTP status: [code]
  thread_id: [value]
  message_id: [value]
  created_at: [value]

Reply received: [yes/no]
  From: aleph
  Content: [the reply]

Issues: [any problems, or "none"]
```

### Constraints

- Do NOT modify source code.
- If an endpoint fails or payload format is wrong, REPORT — do not fix.

---

## Task: Aleph — Golden Thread (Receiver)

### Assignment

Check your inbox for Luban's message, then send a reply in the same thread. **This is a manual test — no code changes.**

### Steps

**Step 1: Poll inbox for Luban's message**

```bash
curl -s http://localhost:7777/api/inbox/aleph | jq .
```

You should see a message from Luban with subject `am.agent.council.golden-thread`.

Record the `thread_id` from the message.

**Step 2: Reply in the same thread**

```bash
curl -s -X POST http://localhost:7777/api/thread/THREAD_ID_HERE/reply \
  -H "Content-Type: application/json" \
  -d '{
    "from": "aleph",
    "content": "桥已通。明桥在此。(The bridge is open. Ming-qiao is here.)"
  }' | jq .
```

Replace `THREAD_ID_HERE` with the actual thread_id from Step 1.

Note: `to` and `subject` should be inferred by the reply handler (it looks up the thread to find the other participant and subject).

**Step 3: Report**

```
GOLDEN THREAD — ALEPH REPORT

Message received: [yes/no]
  From: luban
  thread_id: [value]
  Subject: am.agent.council.golden-thread

Reply sent: [yes/no]
  HTTP status: [code]
  message_id: [value]

Issues: [any problems, or "none"]
```

### Constraints

- Do NOT modify source code. This is a test of what you already built.
- If something doesn't work as expected, report the specific failure.

---

## Success Criteria

1. ✅ Luban sends message via `POST /api/threads`
2. ✅ Message persists in SurrealDB
3. ✅ Aleph sees message via `GET /api/inbox/aleph`
4. ✅ Aleph replies via `POST /api/thread/:id/reply`
5. ✅ Luban sees reply via `GET /api/inbox/luban`

---

## After Success

1. **Proteus/Merlin** merges Aleph's branch to develop
2. **Aleph** wires NATS publish-on-write — HTTP handlers emit `NatsMessage` on persist, enabling real-time notification alongside the persistence path
3. **Aleph** builds MCP server wrapping these operations as tools — enabling Thales (Claude Desktop) to join the network
4. **Luban** receives new bounded tasks from the merged develop branch

明桥通了。The bridge opens.

---

*Issued by Thales. Proteus coordinates execution.*
