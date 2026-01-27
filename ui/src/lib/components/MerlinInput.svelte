<script lang="ts">
  import { api } from '$lib/api';
  import type { InjectAction } from '$lib/types';

  interface Props {
    threadId: string;
  }

  let { threadId }: Props = $props();
  
  let content = $state('');
  let action = $state<InjectAction>('comment');
  let loading = $state(false);
  let showActions = $state(false);

  const actions: InjectAction[] = ['comment', 'pause', 'redirect', 'approve', 'reject'];

  function getActionIcon(act: InjectAction): string {
    switch (act) {
      case 'comment':
        return '💬';
      case 'pause':
        return '⏸️';
      case 'redirect':
        return '↪️';
      case 'approve':
        return '✅';
      case 'reject':
        return '❌';
    }
  }

  function getActionLabel(act: InjectAction): string {
    switch (act) {
      case 'comment':
        return 'Comment';
      case 'pause':
        return 'Pause Thread';
      case 'redirect':
        return 'Redirect';
      case 'approve':
        return 'Approve';
      case 'reject':
        return 'Reject';
    }
  }

  async function handleSubmit() {
    if (!content.trim() || loading) return;

    loading = true;
    try {
      await api.injectMessage({
        thread_id: threadId,
        content: content.trim(),
        action,
      });

      // Clear input on success
      content = '';
      showActions = false;
      
      // Thread will be updated via WebSocket in production
    } catch (e) {
      console.error('Error injecting message:', e);
      alert('Failed to inject message: ' + (e instanceof Error ? e.message : 'Unknown error'));
    } finally {
      loading = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      handleSubmit();
    }
  }
</script>

<div class="merlin-input bg-white border border-gray-200 rounded-lg p-4">
  <div class="flex items-start gap-2 mb-3">
    <div class="w-8 h-8 rounded-full bg-purple-500 flex items-center justify-center text-white font-semibold flex-shrink-0">
      M
    </div>
    <div class="flex-1">
      <p class="text-sm font-medium text-gray-900">Merlin</p>
      <p class="text-xs text-gray-500">Inject message or action</p>
    </div>
  </div>

  <div class="space-y-3">
    <!-- Action Selector -->
    <div class="flex items-center gap-2">
      <button
        class="flex items-center gap-1 px-3 py-1.5 text-sm rounded-md border border-gray-300 hover:bg-gray-50 transition-colors"
        onclick={() => (showActions = !showActions)}
      >
        <span>{getActionIcon(action)}</span>
        <span>{getActionLabel(action)}</span>
        <svg
          class="w-4 h-4 ml-1 {showActions ? 'rotate-180' : ''} transition-transform"
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
        </svg>
      </button>

      {#if showActions}
        <div class="flex gap-1 flex-wrap">
          {#each actions as act}
            <button
              class="px-3 py-1 text-sm rounded-md border {action === act
                ? 'bg-purple-100 border-purple-300 text-purple-700'
                : 'border-gray-300 hover:bg-gray-50'}"
              onclick={() => {
                action = act;
                showActions = false;
              }}
            >
              {getActionIcon(act)} {getActionLabel(act)}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Message Input -->
    <div class="relative">
      <textarea
        bind:value={content}
        onkeydown={handleKeydown}
        disabled={loading}
        placeholder="Type your message... (⌘+Enter to send)"
        class="w-full px-3 py-2 border border-gray-300 rounded-md resize-none focus:outline-none focus:ring-2 focus:ring-purple-500 focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed"
        rows="3"
      ></textarea>
      
      <div class="absolute bottom-2 right-2 text-xs text-gray-400">
        {content.length} chars
      </div>
    </div>

    <!-- Send Button -->
    <div class="flex justify-end">
      <button
        class="px-4 py-2 bg-purple-500 hover:bg-purple-600 text-white rounded-md font-medium disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        disabled={!content.trim() || loading}
        onclick={handleSubmit}
      >
        {#if loading}
          <span class="inline-block animate-spin mr-2">⟳</span>
          Sending...
        {:else}
          <span class="flex items-center gap-1">
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M12 19l9 2-9-18-9 18 9-2zm0 0v-8"
              />
            </svg>
            Send
          </span>
        {/if}
      </button>
    </div>
  </div>
</div>
