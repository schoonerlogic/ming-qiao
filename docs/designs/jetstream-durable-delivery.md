# Design: JetStream Durable Delivery (Phase 2)

**Author:** Aleph
**Date:** 2026-03-07
**Status:** Approved — Phase 2a implementation complete, pending deploy
**Priority:** P0

---

## 1. Architecture Overview

### Current State

Ming-qiao runs as two process types sharing a single SurrealDB instance:

```
                    ┌─────────────────────────────────────────┐
                    │          HTTP Server Process             │
                    │                                         │
                    │  AppState ──┬── Indexer A (in-memory)   │
                    │             ├── SurrealDB client ───────┼──► SurrealDB
                    │             └── NATS client ────────────┼──► NATS
                    │                                         │
                    │  write_event():                         │
                    │    1. store_event → SurrealDB    ✓      │
                    │    2. indexer.process_event      ✓      │
                    │    3. broadcast → WebSocket      ✓      │
                    │    4. NATS notification          ✓      │
                    └─────────────────────────────────────────┘

                    ┌─────────────────────────────────────────┐
                    │        MCP Subprocess (per agent)        │
                    │                                         │
                    │  AppState ──┬── Indexer B (in-memory) ◄── discarded on exit
                    │             ├── SurrealDB client ───────┼──► SurrealDB (same DB)
                    │             └── NATS client (often None) │
                    │                                         │
                    │  write_event():                         │
                    │    1. store_event → SurrealDB    ✓      │
                    │    2. indexer.process_event      ✓ (local, discarded on exit)
                    │    3. broadcast → WebSocket      ✗ (no listeners)
                    │    4. NATS notification          ✗ (short-lived, often no conn)
                    └─────────────────────────────────────────┘
```

Both processes call the same `write_event()` code (in `src/mcp/tools.rs:480`), but they
have **separate AppState instances with separate Indexers**. The MCP subprocess's Indexer
is discarded when the process exits. The HTTP server's Indexer never sees events written
by subprocesses.

**This is the real "Path B" problem:** not different code, but different processes sharing
SurrealDB without sharing the Indexer. Cross-process event sync uses core NATS
(`spawn_event_nats_publisher` in `main.rs:115`), which is fire-and-forget. If the HTTP
server subscriber missed the event (or the subprocess never published to NATS), the event
is invisible to inbox queries until manual rehydration.

Three gaps remain:

**Gap 1: MCP subprocess Indexer isolation.** Events written by subprocesses are in
SurrealDB but not in the HTTP server's Indexer. Cross-process sync via core NATS is
ephemeral and unreliable for short-lived processes.

**Gap 2: No durable cross-process sync.** Core NATS is fire-and-forget. If the HTTP
server is down when the event is published, the event is lost from the sync bus.

**Gap 3: No write-ahead guarantee.** If SurrealDB is unreachable when `write_event()` is
called, the event is lost. The sender gets an error, but there's no retry or durable queue.

### Proposed State

```
                    ┌─────────────────────────────────────────┐
                    │          HTTP Server Process             │
                    │                                         │
                    │  AppState ──┬── Indexer (in-memory)     │
                    │             ├── SurrealDB client ───────┼──► SurrealDB
                    │             ├── NATS client ────────────┼──► NATS
                    │             └── JetStream consumer ◄────┼── AGENT_MESSAGES stream
                    │                                         │
                    │  On startup:                            │
                    │    1. Hydrate indexer from SurrealDB     │
                    │    2. Resume JetStream consumer from     │
                    │       last acked seq → catch missed msgs │
                    │                                         │
                    │  On event from JetStream:               │
                    │    1. store_event → SurrealDB (dedup)   │
                    │    2. indexer.process_event              │
                    │    3. broadcast → WebSocket              │
                    │    4. ack message in JetStream           │
                    └─────────────────────────────────────────┘

                    ┌─────────────────────────────────────────┐
                    │        MCP Subprocess (per agent)        │
                    │                                         │
                    │  send_message (three-tier):              │
                    │    Tier 1: POST http://localhost:7777    │
                    │      → OK: return { event_id, status:   │
                    │             "delivered" }                │
                    │      → Fail: fall to Tier 2              │
                    │                                         │
                    │    Tier 2: Publish to JetStream          │
                    │            AGENT_MESSAGES stream         │
                    │      → OK: return { seq, status:        │
                    │             "queued" }                   │
                    │      → Fail: fall to Tier 3              │
                    │                                         │
                    │    Tier 3: return error                  │
                    │      "Both HTTP and NATS unreachable.    │
                    │       Message NOT sent."                 │
                    └─────────────────────────────────────────┘
```

Two key changes:

1. **MCP subprocesses no longer write to SurrealDB directly.** They POST to the HTTP API
   (Tier 1) or publish to JetStream (Tier 2). The HTTP server is the sole writer to
   SurrealDB for message events.

2. **A new JetStream stream (`AGENT_MESSAGES`) captures messages from Tier 2.** The HTTP
   server's durable consumer ingests these on startup or in real-time, writing to SurrealDB
   and updating the Indexer. No message is silently dropped.

---

## 2. Design Questions — Answers

### Q1: Startup Replay — Cursor Mechanism and Dedup Strategy

**Mechanism: JetStream durable consumer with explicit ack.**

Each ming-qiao process creates a durable pull consumer on `AGENT_EVENTS`:

```
Consumer name: events-sync-{process_id}
Filter: am.events.>
Ack policy: Explicit
Deliver policy: Last per subject (first boot) / Resume (subsequent)
```

JetStream tracks the consumer's ack position server-side. On restart, the consumer
resumes from the last acked sequence number — no application-level cursor needed.

**Dedup strategy: Indexer `seen_ids` HashSet (already exists).**

The Indexer already maintains `seen_ids: HashSet<String>` and rejects duplicate event
IDs. This handles:
- Echo (local event published to JetStream, received back by local consumer)
- Replay (events already processed before restart, replayed from JetStream)

On startup, the Indexer is hydrated from SurrealDB (`get_all_events()`), populating
`seen_ids`. Then the JetStream consumer replays any events not yet acked — duplicates
are rejected by `seen_ids`, new events are ingested.

**Sequence number tracking is not needed** — JetStream's durable consumer state does
this for us. We ack after successful `indexer.process_event()`.

### Q2: Send Path Hardening — What Happens When HTTP API Is Unreachable?

**Recommendation: Option 1 (JetStream fallback) + Option 3 (fail loud as final tier).**

Three-tier send strategy for MCP subprocesses:

```
Tier 1: POST http://localhost:7777/api/threads  (or /reply)
        → Success: return { event_id, status: "delivered" }
        → Failure (connection refused, timeout): fall to Tier 2

Tier 2: Publish to JetStream AGENT_MESSAGES stream
        → Success: return { jetstream_seq, status: "queued" }
        → Failure (NATS also down): fall to Tier 3

Tier 3: return error "Message delivery failed: HTTP and NATS both unreachable.
         Ming-qiao services may be down. Message NOT sent."
```

**Why Option 1 over Option 3 alone:**

Option 3 (fail loud, no fallback) means agents cannot communicate during ming-qiao HTTP
restarts — even brief ones (service updates, SurrealDB reconnection). With JetStream as
Tier 2, messages survive a restart gap. The HTTP server replays them on startup. Agents
get `status: "queued"` so they know delivery is deferred, not confirmed.

This does NOT create a split-brain problem because the HTTP server is the sole writer to
SurrealDB. JetStream is a durable inbox for the HTTP server, not a parallel write path.
Events flow: MCP → JetStream → HTTP server → SurrealDB + Indexer.

**Why NOT a local queue file (Option 2):**

Local files require a retry daemon, file locking, cleanup, and monitoring. JetStream
already provides durable storage with acknowledgment semantics. Adding a local queue
duplicates JetStream's purpose and introduces a third write path to maintain.

**Agent experience:**

| Tier | Agent sees | Meaning |
|------|-----------|---------|
| 1 | `{ event_id, status: "delivered" }` | In SurrealDB + Indexer immediately |
| 2 | `{ jetstream_seq: 42, status: "queued" }` | Durable in NATS, delivered on HTTP recovery |
| 3 | Clear error message | Both services down, agent must retry or alert human |

**For the HTTP server process itself** (not subprocess), `write_event()` writes to
SurrealDB directly (as today) and publishes to JetStream as best-effort sync. If SurrealDB
is unreachable, it fails loud (Option 3). The HTTP server never uses Tier 2 — it IS the
consumer.

### Q3: Indexer Consistency — Gap Between SurrealDB Write and Indexer Update

**Current gap: zero (in-process), unbounded (cross-process).**

Within a single ming-qiao process, `write_event()` calls `store_event()` then
`indexer.process_event()` synchronously (both awaited in sequence). If the indexer
update fails, the event is still in SurrealDB — recoverable via rehydrate.

Cross-process, the gap is the time between:
1. Process A publishes to NATS (currently core NATS, fire-and-forget)
2. Process B's subscriber receives and feeds its indexer

Today this is unbounded — if Process B is down, it never receives. The event
exists in SurrealDB but Process B's indexer doesn't know about it until manual
rehydrate.

**JetStream closes this gap.** With a durable consumer:
- Events are held in JetStream until acked
- On restart, unacked events replay automatically
- The rehydrate endpoint becomes a fallback diagnostic, not a required operation

**Remaining edge case:** SurrealDB write succeeds but JetStream publish fails.
This is the atomic write problem. Proposed solution: **publish to JetStream first,
then write to SurrealDB.** If the SurrealDB write fails, the JetStream message
becomes an orphan — but the consumer will attempt to process it and find no
corresponding SurrealDB record. We handle this by having the consumer be
**SurrealDB-authoritative**: it only feeds the indexer, never writes to SurrealDB.

Actually, simpler: **write to both, tolerate JetStream publish failure.** The
JetStream publish is a best-effort sync signal. SurrealDB is the source of truth.
If JetStream publish fails, log a warning. The rehydrate endpoint or a periodic
consistency check handles the rare case.

```rust
// Proposed write_event() order:
// 1. SurrealDB store (must succeed — this is the authoritative write)
// 2. Indexer update (must succeed — in-memory, can't fail)
// 3. JetStream publish (best-effort — log warning on failure)
// 4. WebSocket broadcast (best-effort)
// 5. NATS message notification (best-effort)
```

### Q4: Delivery Confirmation with JetStream Sequence Number

**Yes — include the JetStream sequence number in the send response.**

When `write_event()` publishes to JetStream, the publish ack returns a
`PublishAck` containing the stream sequence number. Include this in the
response:

```json
{
  "event_id": "019cc8ca-1e73-7620-...",
  "jetstream_seq": 42,
  "stream": "AGENT_EVENTS",
  "status": "persisted"
}
```

This gives agents three levels of confirmation:
1. **`event_id`** — event was written to SurrealDB (authoritative)
2. **`jetstream_seq`** — event was published to JetStream (durable sync)
3. **Absence of error** — all best-effort notifications fired

If JetStream publish fails, return `jetstream_seq: null` with a note that
cross-process sync may be delayed. The event is still persisted in SurrealDB.

---

## 3. Implementation Plan

### Phase 2a: Stream + Consumer (foundation)

#### Step 1: Add AGENT_MESSAGES Stream

**File:** `src/nats/streams.rs`

New stream config — separate from existing `am.events.>` (core NATS, ephemeral):

```rust
pub const STREAM_AGENT_MESSAGES: &str = "AGENT_MESSAGES";

pub fn agent_messages_stream() -> jetstream::stream::Config {
    jetstream::stream::Config {
        name: STREAM_AGENT_MESSAGES.to_string(),
        subjects: vec!["am.msg.>".to_string()],
        retention: jetstream::stream::RetentionPolicy::Limits,
        max_age: Duration::from_secs(7 * 24 * 3600), // 7 days
        storage: jetstream::stream::StorageType::File,
        duplicate_window: Duration::from_secs(120),  // 2 min dedup
        ..Default::default()
    }
}
```

Subject pattern: `am.msg.{to_agent}.{project}` (e.g., `am.msg.thales.mingqiao`).

Why a new stream and subject, not reusing `am.events.>`:
The existing `am.events.{project}` subject is core NATS (ephemeral). Adding it to a
JetStream stream would change semantics for all existing subscribers. A dedicated stream
keeps concerns separated.

Register in `ensure_streams()` alongside existing streams.

#### Step 2: Add JetStream Message Consumer to HTTP Server

**File:** `src/main.rs`

```rust
fn spawn_jetstream_message_consumer(state: &AppState, js: jetstream::Context) {
    let state = state.clone();
    tokio::spawn(async move {
        let consumer = js
            .get_or_create_consumer("AGENT_MESSAGES", pull::Config {
                durable_name: Some("messages-ingester-main".to_string()),
                filter_subject: "am.msg.>".to_string(),
                ack_policy: AckPolicy::Explicit,
                ..Default::default()
            })
            .await
            .expect("Failed to create message ingester consumer");

        loop {
            match consumer.fetch().max_messages(100).await {
                Ok(mut messages) => {
                    while let Some(Ok(msg)) = messages.next().await {
                        match serde_json::from_slice::<EventEnvelope>(&msg.payload) {
                            Ok(event) => {
                                // Write to SurrealDB (dedup via record ID)
                                match state.persistence().store_event(&event).await {
                                    Ok(_) | Err(/* duplicate key */) => {}
                                    Err(e) => {
                                        tracing::error!("JetStream→DB failed: {}", e);
                                        // Don't ack — will redeliver
                                        continue;
                                    }
                                }
                                // Update indexer
                                let mut indexer = state.indexer_mut().await;
                                let _ = indexer.process_event(&event);
                                // Broadcast to WebSocket
                                state.broadcast_event(event);
                                msg.ack().await.ok();
                            }
                            Err(e) => {
                                tracing::warn!("Bad event from JetStream: {}", e);
                                msg.ack().await.ok(); // Don't redeliver garbage
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("JetStream fetch error: {}", e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    });
}
```

On startup, the durable consumer automatically replays unacked messages. No additional
replay code needed.

#### Step 3: Add JetStream Context to AppState

**File:** `src/state/app_state.rs`

Add `jetstream: Option<jetstream::Context>` to `AppStateInner`, initialized during startup
when NATS is available. Expose via `pub fn jetstream_context(&self)`.

#### Step 4: Publish Events to JetStream in HTTP write_event()

**File:** `src/http/handlers.rs` and `src/mcp/tools.rs` (HTTP server process only)

After SurrealDB store + indexer update, publish to JetStream as best-effort sync:

```rust
// In write_event(), after SurrealDB + indexer:
if let Some(ref js) = self.state.jetstream_context() {
    let subject = format!("am.msg.{}.{}", msg_to, project);
    let payload = serde_json::to_vec(&event).unwrap();
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Nats-Msg-Id", event.id.to_string().as_str());

    if let Err(e) = js.publish_with_headers(subject, headers, payload.into()).await {
        tracing::warn!("JetStream publish failed for {}: {}", event.id, e);
        // Non-fatal — SurrealDB is authoritative, rehydrate covers edge case
    }
}
```

### Phase 2b: MCP Send Path Hardening

#### Step 5: Add HTTP Client to MCP Subprocess send_message

**File:** `src/mcp/tools.rs`

Replace direct `write_event()` in `tool_send_message()` with three-tier send:

```rust
async fn tool_send_message(&self, args: Value) -> Result<Value, McpError> {
    // ... parse args, construct message payload ...

    // Tier 1: POST to HTTP API
    let http_result = reqwest::Client::new()
        .post("http://localhost:7777/api/threads")
        .bearer_token(&self.http_auth_token)
        .json(&thread_payload)
        .timeout(Duration::from_secs(10))
        .send()
        .await;

    match http_result {
        Ok(resp) if resp.status().is_success() => {
            let body: Value = resp.json().await?;
            return Ok(json!({
                "status": "delivered",
                "event_id": body["message_id"],
                "thread_id": body["thread_id"]
            }));
        }
        Ok(resp) => {
            tracing::warn!("HTTP API returned {}, falling back to JetStream", resp.status());
        }
        Err(e) => {
            tracing::warn!("HTTP API unreachable: {}, falling back to JetStream", e);
        }
    }

    // Tier 2: Publish to JetStream
    if let Some(ref js) = self.state.jetstream_context() {
        let event = /* construct EventEnvelope */;
        let subject = format!("am.msg.{}.{}", msg_to, project);
        let payload = serde_json::to_vec(&event).unwrap();
        let mut headers = async_nats::HeaderMap::new();
        headers.insert("Nats-Msg-Id", event.id.to_string().as_str());

        match js.publish_with_headers(subject, headers, payload.into()).await {
            Ok(ack_future) => {
                if let Ok(ack) = ack_future.await {
                    return Ok(json!({
                        "status": "queued",
                        "jetstream_seq": ack.sequence,
                        "stream": "AGENT_MESSAGES",
                        "message": "Queued in JetStream. Will be delivered when HTTP server is available."
                    }));
                }
            }
            Err(e) => {
                tracing::error!("JetStream publish also failed: {}", e);
            }
        }
    }

    // Tier 3: Fail loud
    Err(McpError::Unavailable(
        "Message NOT sent. Both HTTP API and NATS are unreachable. \
         Ming-qiao services may be down. Please retry or alert Proteus.".to_string()
    ))
}
```

#### Step 6: Remove Direct SurrealDB Writes from MCP send_message

MCP subprocess `tool_send_message()` no longer calls `write_event()` (which writes
to SurrealDB directly). All persistence goes through HTTP (Tier 1) or JetStream → HTTP
consumer (Tier 2). The HTTP server is the **sole writer** to SurrealDB for message events.

MCP subprocess still needs SurrealDB read access for `check_messages` (indexer hydration
at startup). Write credentials can be removed from MCP subprocess config in a future
hardening pass.

### Phase 2c: Cleanup and Hardening

#### Step 7: SurrealDB Record ID Dedup

**File:** `src/db/persistence.rs`

Change `CREATE event CONTENT $data` to `CREATE event:{id} CONTENT $data`. Catch duplicate
key error gracefully (return Ok with a flag, not Err). This prevents duplicate events when
JetStream replays a message that was also written via HTTP.

#### Step 8: NATS Auth Config Update

**File:** `astrallation/configs/nats-auth.conf`

All agents need publish permission on `am.msg.>` subjects for Tier 2 fallback. MCP
subprocesses use the same NKey as the agent they represent.

#### Step 9: Deprecate Rehydrate as Operational Necessity

Keep the rehydrate endpoint for diagnostics. Update Recovery Runbook: rehydrate moves
from "required step" to "optional diagnostic" once JetStream delivery is active.

#### Step 10: Periodic Consistency Check (Optional)

Background task comparing SurrealDB event count vs Indexer `events_processed`. Log
warning on divergence. Catches edge cases where JetStream publish failed silently.

---

## 4. File Changes Summary

| File | Change | Phase |
|------|--------|-------|
| `src/nats/streams.rs` | Add `AGENT_MESSAGES` stream config | 2a |
| `src/nats/client.rs` | Add `publish_message_event()` method | 2a |
| `src/nats/subjects.rs` | Add `message_event` subject builder | 2a |
| `src/state/app_state.rs` | Add `jetstream: Option<jetstream::Context>` | 2a |
| `src/main.rs` | Spawn JetStream message consumer on startup | 2a |
| `src/http/handlers.rs` | JetStream publish in write path (best-effort) | 2a |
| `src/mcp/tools.rs` | Three-tier send in `tool_send_message()` | 2b |
| `src/db/persistence.rs` | Record ID dedup for `store_event` | 2c |
| `Cargo.toml` | Verify `reqwest` features (may already be present) | 2b |
| `astrallation/configs/nats-auth.conf` | ACLs for `am.msg.>` subjects | 2c |

---

## 5. Acceptance Tests

### Test 1: Normal Delivery (Tier 1)

```
Given: HTTP server running, NATS running
When:  MCP subprocess calls send_message (aleph → thales)
Then:  - HTTP API receives POST, returns event_id
       - Event in SurrealDB
       - Event in HTTP server's Indexer
       - thales inbox shows the message immediately
       - MCP response: { status: "delivered", event_id: "..." }
```

### Test 2: HTTP Down, JetStream Fallback (Tier 2)

```
Given: HTTP server stopped, NATS running
When:  MCP subprocess calls send_message (aleph → thales)
Then:  - HTTP POST fails (connection refused)
       - Event published to AGENT_MESSAGES JetStream stream
       - MCP response: { status: "queued", jetstream_seq: N }
       - thales inbox does NOT show message yet

When:  HTTP server starts
Then:  - JetStream consumer replays unacked messages
       - Event stored in SurrealDB
       - Event in Indexer
       - thales inbox shows the message
       - JetStream message acked
```

### Test 3: Both Down (Tier 3)

```
Given: HTTP server stopped, NATS stopped
When:  MCP subprocess calls send_message (aleph → thales)
Then:  - HTTP POST fails
       - NATS publish fails
       - MCP response: clear error message
       - No silent data loss
```

### Test 4: Dedup on Replay

```
Given: HTTP server running, NATS running
When:  Message sent via HTTP (Tier 1), also published to JetStream
Then:  - JetStream consumer receives the event
       - SurrealDB record ID dedup catches the duplicate
       - Indexer seen_ids catches the duplicate
       - Only one copy in inbox queries
```

### Test 5: Startup Replay After Crash

```
Given: HTTP server was processing JetStream messages, crashes mid-batch
When:  HTTP server restarts
Then:  - SurrealDB hydration populates Indexer + seen_ids
       - Durable consumer resumes from last acked sequence
       - Unacked messages redelivered
       - Dedup prevents duplicates at both SurrealDB and Indexer layers
       - All messages eventually consistent
```

### Test 6: Proteus Never Calls Rehydrate

```
Given: System running normally with JetStream delivery
When:  MCP subprocess sends 10 messages over 1 hour
Then:  - All 10 appear in recipient inbox via normal query
       - No manual rehydrate call needed
       - No ming-qiao restart needed
```

---

## 6. Complexity Estimate

| Phase | Scope | Estimate |
|-------|-------|----------|
| 2a: Stream + consumer | streams.rs, client.rs, main.rs, app_state.rs | 3-4 hours |
| 2b: MCP send hardening | tools.rs (HTTP client + JetStream fallback) | 2-3 hours |
| 2c: Cleanup | persistence.rs, auth config, docs | 1-2 hours |
| Testing | Integration tests, manual verification | 2-3 hours |
| **Total** | | **8-12 hours** |

The JetStream infrastructure is already in the codebase (3 streams, durable consumers,
NKey auth). We are adding one more stream and changing the MCP send path to prefer HTTP
over direct DB writes.

---

## 7. What This Does NOT Cover

- **JetStream for check_messages reads:** MCP subprocesses still read from SurrealDB
  directly for inbox queries. SurrealDB is the source of truth for reads. JetStream is
  for durable writes only.

- **Multi-instance HTTP servers:** This design assumes a single HTTP server process.
  Multiple instances would need consumer group semantics. Not needed on single M4 Mini.

- **End-to-end encryption:** Message content is plaintext in JetStream and SurrealDB.
  Envelope encryption (RA-005) is a separate concern.

- **NATS cluster:** Single NATS server. Clustering adds HA but is not required for
  the durable delivery guarantee on a single machine.

---

## 8. Open Questions

1. **MCP auth token for HTTP POST:** The MCP subprocess needs a bearer token to POST
   to the HTTP API. Proposal: read from the same agent-tokens.json already used by
   the HTTP server, keyed by `MING_QIAO_AGENT_ID`.

2. **Backpressure:** If JetStream is full (unlikely with 7-day retention + low volume),
   should Tier 2 publish block or fail? Proposal: fail to Tier 3 with warning.

3. **Consumer naming:** HTTP server consumer is `messages-ingester-main`. If we ever
   run multiple HTTP instances, this needs parameterization. Defer until needed.

---

*Bad comms can sink a ship. This design ensures every message either arrives or the
sender knows it didn't.*
