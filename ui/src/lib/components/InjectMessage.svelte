<script lang="ts">
  import { merlinNotifications } from '$stores/merlinNotifications';
  import type { ThreadDetail } from '$lib/types';
  
  interface Props {
    open: boolean;
    thread: ThreadDetail | null;
    onClose?: () => void;
  }
  
  let { open, thread, onClose }: Props = $props();
  
  let content = $state('');
  let sending = $state(false);
  const maxChars = 2000;
  
  // Close modal on escape key
  $effect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && open) {
        open = false;
      }
    };
    
    if (open) {
      window.addEventListener('keydown', handleEscape);
      return () => window.removeEventListener('keydown', handleEscape);
    }
  });
  
  function close() {
    open = false;
    content = '';
    onClose?.();
  }
  
  function submit() {
    if (!content.trim() || !thread || sending) return;
    
    sending = true;
    
    const success = merlinNotifications.sendIntervention({
      action: 'inject_message',  // Backend expects snake_case
      threadId: thread.thread_id,
      from: 'merlin',
      content: content.trim()
    });
    
    if (success) {
      merlinNotifications.showToast({
        type: 'success',
        message: 'Message injected successfully',
        duration: 3000
      });
      close();
    } else {
      sending = false;
    }
  }
  
  function handleTextareaKeydown(e: KeyboardEvent) {
    // Submit on Cmd+Enter / Ctrl+Enter
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      submit();
    }
  }
</script>

{#if open && thread}
  <div
    class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50"
    onclick={(e) => {
      if (e.target === e.currentTarget) close();
    }}
    role="dialog"
    aria-modal="true"
    aria-labelledby="inject-title"
  >
    <div class="bg-white rounded-lg shadow-xl max-w-2xl w-full mx-4 max-h-[90vh] overflow-y-auto">
      <!-- Header -->
      <div class="px-6 py-4 border-b border-gray-200">
        <div class="flex items-center justify-between">
          <div>
            <h2 id="inject-title" class="text-xl font-semibold text-gray-900">
              ⚡ Inject Message
            </h2>
            <p class="text-sm text-gray-600 mt-1">
              Inject a message into this thread as Merlin
            </p>
          </div>
          <button
            onclick={close}
            class="text-gray-400 hover:text-gray-600 transition-colors"
            aria-label="Close"
          >
            <svg class="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M6 18L18 6M6 6l12 12"
              />
            </svg>
          </button>
        </div>
        
        <!-- Thread Context -->
        <div class="mt-4 p-3 bg-gray-50 rounded-md">
          <div class="text-sm font-medium text-gray-900">{thread.subject}</div>
          <div class="text-xs text-gray-600 mt-1">
            Participants: {thread.participants.join(', ')}
          </div>
        </div>
      </div>
      
      <!-- Body -->
      <div class="px-6 py-4">
        <label for="message-content" class="block text-sm font-medium text-gray-700 mb-2">
          Message
        </label>
        <textarea
          id="message-content"
          bind:value={content}
          onkeydown={handleTextareaKeydown}
          disabled={sending}
          class="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent resize-none"
          rows="8"
          placeholder="Enter your message..."
          maxlength={maxChars}
        ></textarea>
        
        <!-- Character Counter -->
        <div class="flex justify-between items-center mt-2">
          <span class="text-xs text-gray-500">
            {content.length} / {maxChars} characters
          </span>
          <span class="text-xs text-gray-500">
            Press <kbd class="px-1 py-0.5 bg-gray-100 border border-gray-300 rounded">⌘+Enter</kbd> to submit
          </span>
        </div>
      </div>
      
      <!-- Footer -->
      <div class="px-6 py-4 border-t border-gray-200 flex justify-end gap-3">
        <button
          onclick={close}
          disabled={sending}
          class="px-4 py-2 text-gray-700 bg-gray-100 hover:bg-gray-200 rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        >
          Cancel
        </button>
        <button
          onclick={submit}
          disabled={!content.trim() || sending}
          class="px-4 py-2 text-white bg-blue-600 hover:bg-blue-700 rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2"
        >
          {#if sending}
            <svg class="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24">
              <circle
                class="opacity-25"
                cx="12"
                cy="12"
                r="10"
                stroke="currentColor"
                stroke-width="4"
              ></circle>
              <path
                class="opacity-75"
                fill="currentColor"
                d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
              ></path>
            </svg>
            Injecting...
          {:else}
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M13 10V3L4 14h7v7l9-11h-7z"
              />
            </svg>
            Inject Message
          {/if}
        </button>
      </div>
    </div>
  </div>
{/if}
