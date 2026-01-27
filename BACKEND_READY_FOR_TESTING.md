# Backend Verification Report for Luban's UI Testing

**Date:** 2026-01-27
**Verified By:** Aleph
**Status:** ✅ READY FOR TESTING

---

## Server Status

**HTTP Server:** Running on `http://localhost:7777`
**WebSocket Events:** `ws://localhost:7777/ws`
**WebSocket Merlin:** `ws://localhost:7777/merlin/notifications`

**Command to start:**
```bash
./target/debug/ming-qiao serve
```

---

## API Endpoints Available

### Thread & Message APIs
- ✅ `GET /api/threads` — List all threads
- ✅ `GET /api/threads/{id}` — Get thread details
- ✅ `GET /api/threads/{id}/messages` — Get thread messages
- ✅ `GET /api/messages` — List all messages
- ✅ `GET /api/messages/{id}` — Get single message

### Decision APIs
- ✅ `GET /api/decisions` — List all decisions
- ✅ `GET /api/decisions/{id}` — Get single decision

### Artifact APIs
- ✅ `GET /api/artifacts` — List all artifacts
- ✅ `GET /api/artifacts/{id}` — Get single artifact

### Merlin WebSocket
- ✅ `WS /merlin/notifications` — Real-time intervention stream

---

## Test Data Available

**Event Log:** `data/events.jsonl`
- Total events: 18
- Last event: Merlin intervention (injectMessage)
- Contains: Threads, messages, decisions, artifacts

**Sample Thread:**
```
ID: 019c00c8-129d-77f2-ac1c-a6a9ff098d15
Subject: Test
Participants: aleph, thales, merlin
Messages: 2
Status: active
```

---

## Merlin Interventions Supported

### 1. injectMessage ✅ FULLY WORKING
```javascript
// Send via WebSocket
{
  "action": "injectMessage",
  "threadId": "019c00c8-129d-77f2-ac1c-a6a9ff098d15",
  "from": "merlin",
  "content": "Test message from Merlin"
}
```
**Expected Result:**
- Message written to event log
- Message appears in UI immediately
- Toast notification: "Message injected successfully"

### 2. setMode ✅ FULLY WORKING
```javascript
{
  "action": "setMode",
  "mode": "advisory"
}
```
**Expected Result:**
- Mode updated in-memory
- Subsequent connections see new mode
- Toast notification: "Mode changed to advisory"

**Modes:** `passive` | `advisory` | `gated`

### 3. approveDecision ⚠️ PARTIAL
```javascript
{
  "action": "approveDecision",
  "decisionId": "019c00c8-129d-77f2-ac1c-a6a9ff098d15"
}
```
**Current Behavior:**
- Logs approval to console
- Does NOT create DecisionApproved event (TODO)

### 4. rejectDecision ⚠️ PARTIAL
```javascript
{
  "action": "rejectDecision",
  "decisionId": "019c00c8-129d-77f2-ac1c-a6a9ff098d15"
}
```
**Current Behavior:**
- Logs rejection to console
- Does NOT create DecisionRejected event (TODO)

---

## WebSocket Connection Testing

### Test Script (save as /tmp/test_websocket.html)
```html
<!DOCTYPE html>
<html>
<head><title>Merlin WebSocket Test</title></head>
<body>
  <h1>Merlin Notification Stream</h1>
  <div id="status">Connecting...</div>
  <div id="messages"></div>

  <script>
    const ws = new WebSocket('ws://localhost:7777/merlin/notifications');

    ws.onopen = () => {
      document.getElementById('status').textContent = '✅ Connected';
      document.getElementById('status').style.color = 'green';
    };

    ws.onmessage = (event) => {
      const msg = document.createElement('div');
      msg.textContent = `Received: ${event.data}`;
      document.getElementById('messages').appendChild(msg);
    };

    ws.onerror = (error) => {
      document.getElementById('status').textContent = '❌ Error';
      document.getElementById('status').style.color = 'red';
    };

    ws.onclose = () => {
      document.getElementById('status').textContent = '❌ Disconnected';
      document.getElementById('status').style.color = 'red';
    };

    // Test injectMessage
    function sendTest() {
      ws.send(JSON.stringify({
        action: 'injectMessage',
        threadId: '019c00c8-129d-77f2-ac1c-a6a9ff098d15',
        from: 'merlin',
        content: 'Test from browser'
      }));
    }
  </script>

  <button onclick="sendTest()">Send Test Message</button>
</body>
</html>
```

**To test:**
1. Open file in browser: `file:///tmp/test_websocket.html`
2. Should see "✅ Connected"
3. Click button → should see message in UI

---

## Known Limitations

1. **Decision approval/rejection**
   - Works: WebSocket receives intervention
   - Works: Backend logs to console
   - Missing: Event creation (DecisionApproved/DecisionRejected)
   - Impact: Decisions can't be persisted or updated
   - Priority: Medium (documented in SESSION_STATE)

2. **WebSocket auto-reconnect**
   - Implemented in UI (5 second delay)
   - Works on server restart
   - May show console error on normal close (cosmetic)

---

## Verification Checklist for Luban

Before starting UI testing, verify:

- [ ] Server is running: `./target/debug/ming-qiao serve`
- [ ] Can access: `http://localhost:7777/api/threads`
- [ ] Event log exists: `data/events.jsonl` (18 events)
- [ ] Sample thread exists (ID: 019c00c8-129d-77f2-ac1c-a6a9ff098d15)
- [ ] UI dev server ready: `cd ui && npm run dev`

---

## What to Test

### Critical Path (Must Work)
1. **Thread list loads** → GET /api/threads
2. **Thread detail loads** → GET /api/threads/{id}/messages
3. **Mode toggle** → WebSocket setMode
4. **Inject message** → WebSocket injectMessage
5. **Real-time updates** → Two browser tabs

### Nice to Have
6. **Notification center** → Badge count, drawer
7. **Error handling** → Disconnect/reconnect
8. **Decision actions** → Approve/reject buttons

---

## Troubleshooting

**Server won't start:**
```bash
# Check if port 7777 is in use
lsof -i :7777
# Kill existing process
pkill -f "ming-qiao serve"
```

**No threads appearing:**
```bash
# Check event log
wc -l data/events.jsonl
# Verify indexer is running
curl http://localhost:7777/api/threads | jq '.threads | length'
```

**WebSocket not connecting:**
```bash
# Check browser console for errors
# Verify server logs show WebSocket upgrade
```

**Events not appearing:**
```bash
# Check event log is being written
tail -f data/events.jsonl
# Verify indexer is processing events
curl http://localhost:7777/api/threads/{id}/messages | jq '.messages | length'
```

---

## Support

If you encounter issues:

1. **Check server logs** — Look for ERROR or WARN messages
2. **Check browser console** — Look for network errors
3. **Document the issue** — Screenshot + steps to reproduce
4. **Report in COUNCIL_CHAT.md** — I'll review and fix

**Server logs location:** Terminal window running `ming-qiao serve`

---

## Next Steps

1. **Start backend:** `./target/debug/ming-qiao serve`
2. **Start UI:** `cd ui && npm run dev`
3. **Open browser:** `http://localhost:5173`
4. **Run test checklist** (see COUNCIL_CHAT.md task assignment)
5. **Document results** in `docs/UI_TEST_REPORT.md`

---

**Backend Status:** ✅ READY
**Test Data:** ✅ AVAILABLE
**WebSocket:** ✅ LISTENING
**Ready for Luban:** ✅ YES

Good luck with testing! — Aleph
