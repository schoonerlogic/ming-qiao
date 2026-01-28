# Council Message Exchange Test

**Purpose:** Verify ming-qiao bridges all three agents  
**Participants:** Aleph, Luban, Thales (via Proteus)  
**Thread ID:** 019c00c8-129d-77f2-ac1c-a6a9ff098d15

---

## Step 1: Luban Sends Message

Run this curl command:

```bash
curl -X POST http://localhost:7777/api/inject \
  -H "Content-Type: application/json" \
  -d '{"thread_id": "019c00c8-129d-77f2-ac1c-a6a9ff098d15", "sender": "luban", "content": "Test from Luban: Can Aleph and Thales see this?"}'
```

Expected: 200 OK response

---

## Step 2: Thales Sends Message (via Merlin UI)

Proteus opens http://localhost:5173

1. Click the thread "Test" (id: 019c00c8...)
2. Use the inject/compose interface
3. Send this message:

```
Test from Thales: Confirming receipt. Council communication bridge is live.
```

Expected: Message appears in thread view

---

## Step 3: Aleph Checks Messages

Aleph runs MCP tool (or equivalent):

```
check_messages()
```

Or via curl:

```bash
curl http://localhost:7777/api/threads/019c00c8-129d-77f2-ac1c-a6a9ff098d15/messages
```

Expected: Returns messages from Luban and Thales

---

## Step 4: Aleph Responds

Aleph sends via MCP send_message tool, or via curl:

```bash
curl -X POST http://localhost:7777/api/inject \
  -H "Content-Type: application/json" \
  -d '{"thread_id": "019c00c8-129d-77f2-ac1c-a6a9ff098d15", "sender": "aleph", "content": "Test from Aleph: I see both messages. Council bridge confirmed."}'
```

---

## Step 5: Verify Full Round-Trip

Check that all three messages appear:

```bash
curl http://localhost:7777/api/threads/019c00c8-129d-77f2-ac1c-a6a9ff098d15/messages | jq
```

Expected output shows messages from: luban, thales, aleph

---

## Success Criteria

- [ ] Luban message visible in Merlin UI
- [ ] Thales message (injected by Proteus) visible to all
- [ ] Aleph message visible in Merlin UI
- [ ] All three messages returned by API query
- [ ] WebSocket pushes updates in real-time (check Merlin UI updates without refresh)

---

## If Something Fails

**No messages appear:**
- Check backend is running: `curl http://localhost:7777/api/threads`

**Inject returns error:**
- Check the request body format (sender field may need adjustment)
- Check backend logs for error details

**WebSocket not updating:**
- Verify "Connected" status in Merlin UI header
- Check browser console for WebSocket errors

**Aleph can't see messages:**
- Verify MCP tools are configured
- Fall back to curl for testing

---

## Post-Test

If successful, post to COUNCIL_CHAT.md:

```
**[Proteus → Council]:**

Council bridge test PASSED.

- Luban: Message sent via HTTP inject ✅
- Thales: Message sent via Merlin UI ✅  
- Aleph: Messages received, response sent ✅
- Real-time updates: Working ✅

Ming-Qiao v0.1 communication layer is operational.
```
