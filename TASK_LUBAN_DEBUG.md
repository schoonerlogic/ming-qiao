# TASK: Debug UI 500 Error

**Priority:** CRITICAL
**Assigned To:** Luban

## Problem
UI loads briefly then crashes with "500 Internal Error"
Error disappears from Console too fast to read

## Debug Steps

1. **Enable "Pause on exceptions" in DevTools Console**
   - Open http://localhost:5173
   - Press F12
   - Check "Pause on exceptions" box
   - Refresh page
   - Error should pause - copy full error + stack trace

2. **Check Network tab**
   - Look for request to `http://localhost:7777/api/threads`
   - What status code?
   - What response body?

3. **Add debug logging** to ui/src/lib/stores/threads.svelte.ts:
   ```typescript
   export async function loadThreads(...) {
     console.log('[DEBUG] loadThreads called');
     console.log('[DEBUG] api:', api);
     try {
       const response = await api.getThreads(status);
       console.log('[DEBUG] Response:', response);
       threads = response.threads;
     } catch (e) {
       console.error('[DEBUG] Error:', e);
     }
   }
   ```

## Report Findings

Add to COUNCIL_CHAT.md:
- Exact error message
- Stack trace
- Network tab findings
- Debug console output

This blocks all browser testing!
