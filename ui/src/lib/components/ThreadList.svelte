<script lang="ts">
  import { threadsStore, loadThreads } from '$stores/threads.svelte';
  import type { Thread } from '$lib/types';

  interface Props {
    onSelectThread?: (threadId: string) => void;
  }

  let { onSelectThread }: Props = $props();

  let status = $state<'active' | 'paused' | 'resolved' | 'archived' | 'all'>('active');

  async function handleStatusChange(newStatus: typeof status) {
    status = newStatus;
    await loadThreads(newStatus);
  }

  function getStatusBadgeClass(status: string): string {
    const base = 'px-2 py-1 rounded-full text-xs font-medium ';
    switch (status) {
      case 'active':
        return base + 'bg-green-100 text-green-800';
      case 'paused':
        return base + 'bg-yellow-100 text-yellow-800';
      case 'resolved':
        return base + 'bg-blue-100 text-blue-800';
      case 'archived':
        return base + 'bg-gray-100 text-gray-800';
      default:
        return base + 'bg-gray-100 text-gray-800';
    }
  }

  function formatDate(dateStr: string): string {
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMs / 3600000);
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffMins < 1) return 'just now';
    if (diffMins < 60) return `${diffMins}m ago`;
    if (diffHours < 24) return `${diffHours}h ago`;
    if (diffDays < 7) return `${diffDays}d ago`;
    return date.toLocaleDateString();
  }
</script>

<div class="thread-list">
  <div class="flex items-center justify-between mb-4">
    <h2 class="text-2xl font-bold text-gray-900">Threads</h2>
    <div class="flex gap-2">
      <button
        class="px-3 py-1 text-sm rounded-md {status === 'active'
          ? 'bg-blue-500 text-white'
          : 'bg-gray-200 text-gray-700 hover:bg-gray-300'}"
        onclick={() => handleStatusChange('active')}
      >
        Active
      </button>
      <button
        class="px-3 py-1 text-sm rounded-md {status === 'paused'
          ? 'bg-blue-500 text-white'
          : 'bg-gray-200 text-gray-700 hover:bg-gray-300'}"
        onclick={() => handleStatusChange('paused')}
      >
        Paused
      </button>
      <button
        class="px-3 py-1 text-sm rounded-md {status === 'resolved'
          ? 'bg-blue-500 text-white'
          : 'bg-gray-200 text-gray-700 hover:bg-gray-300'}"
        onclick={() => handleStatusChange('resolved')}
      >
        Resolved
      </button>
      <button
        class="px-3 py-1 text-sm rounded-md {status === 'all'
          ? 'bg-blue-500 text-white'
          : 'bg-gray-200 text-gray-700 hover:bg-gray-300'}"
        onclick={() => handleStatusChange('all')}
      >
        All
      </button>
    </div>
  </div>

  {#if threadsStore.loading}
    <div class="flex items-center justify-center py-8">
      <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500"></div>
      <span class="ml-2 text-gray-600">Loading threads...</span>
    </div>
  {:else if threadsStore.error}
    <div class="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-md">
      Error loading threads: {threadsStore.error}
    </div>
  {:else if threadsStore.threads.length === 0}
    <div class="text-center py-8 text-gray-500">
      <p class="text-lg">No threads found</p>
      <p class="text-sm">Create a new thread to get started</p>
    </div>
  {:else}
    <div class="space-y-2">
      {#each threadsStore.threads as thread (thread.thread_id)}
        <div
          class="bg-white border border-gray-200 rounded-lg p-4 hover:border-blue-300 hover:shadow-md transition-all cursor-pointer"
          onclick={() => onSelectThread?.(thread.thread_id)}
        >
          <div class="flex items-start justify-between">
            <div class="flex-1">
              <div class="flex items-center gap-2 mb-1">
                <h3 class="font-semibold text-gray-900">{thread.subject}</h3>
                {#if thread.unread_count > 0}
                  <span class="bg-red-500 text-white text-xs px-2 py-0.5 rounded-full">
                    {thread.unread_count} unread
                  </span>
                {/if}
              </div>
              <div class="flex items-center gap-3 text-sm text-gray-600 mb-2">
                <span class="flex items-center gap-1">
                  <svg
                    class="w-4 h-4"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      stroke-width="2"
                      d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z"
                    />
                  </svg>
                  {thread.participants.join(', ')}
                </span>
                <span class="flex items-center gap-1">
                  <svg
                    class="w-4 h-4"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      stroke-width="2"
                      d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z"
                    />
                  </svg>
                  {thread.message_count} messages
                </span>
                {#if thread.decision_count > 0}
                  <span class="flex items-center gap-1">
                    <svg
                      class="w-4 h-4"
                      fill="none"
                      stroke="currentColor"
                      viewBox="0 0 24 24"
                    >
                      <path
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        stroke-width="2"
                        d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
                      />
                    </svg>
                    {thread.decision_count} decisions
                  </span>
                {/if}
              </div>
            </div>
            <div class="flex flex-col items-end gap-2">
              <span class={getStatusBadgeClass(thread.status)}>{thread.status}</span>
              <span class="text-xs text-gray-500">{formatDate(thread.last_message_at)}</span>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>
