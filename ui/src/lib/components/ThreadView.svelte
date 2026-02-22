<script lang="ts">
  import { threadsStore } from '$stores/threads.svelte';
  import Message from './Message.svelte';
  import DecisionCard from './DecisionCard.svelte';
  import MerlinInput from './MerlinInput.svelte';

  function formatDate(dateStr: string): string {
    const date = new Date(dateStr);
    return date.toLocaleString();
  }
</script>

{#if threadsStore.loading}
  <div class="flex items-center justify-center py-8">
    <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500"></div>
    <span class="ml-2 text-gray-600">Loading thread...</span>
  </div>
{:else if threadsStore.error}
  <div class="bg-red-50 border border-red-200 text-red-700 px-4 py-3 rounded-md">
    Error loading thread: {threadsStore.error}
  </div>
{:else if !threadsStore.currentThread}
  <div class="text-center py-8 text-gray-500">
    <p class="text-lg">No thread selected</p>
    <p class="text-sm">Select a thread from the list to view details</p>
  </div>
{:else}
  <div class="thread-view">
    <!-- Thread Header -->
    <div class="bg-white border-b border-gray-200 p-4 mb-4">
      <div class="flex items-start justify-between mb-2">
        <h1 class="text-2xl font-bold text-gray-900">{threadsStore.currentThread.subject}
        </h1>
        <span class="px-3 py-1 rounded-full text-sm font-medium bg-blue-100 text-blue-800">
          {threadsStore.currentThread.status}
        </span>
      </div>
      <div class="flex items-center gap-4 text-sm text-gray-600">
        <span class="flex items-center gap-1">
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z"
            />
          </svg>
          {threadsStore.currentThread.participants.join(', ')}
        </span>
        <span class="flex items-center gap-1">
          <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z"
            />
          </svg>
          Started {formatDate(threadsStore.currentThread.started_at)}
        </span>
      </div>
    </div>

    <!-- Messages -->
    <div class="space-y-4 mb-6">
      <h2 class="text-lg font-semibold text-gray-900">Messages</h2>
      {#if threadsStore.currentThread.messages.length === 0}
        <p class="text-gray-500 text-center py-4">No messages yet</p>
      {:else}
        {#each threadsStore.currentThread.messages as message (message.message_id)}
          <Message {message} />
        {/each}
      {/if}
    </div>

    <!-- Decisions -->
    {#if threadsStore.currentThread.decisions.length > 0}
      <div class="space-y-4 mb-6">
        <h2 class="text-lg font-semibold text-gray-900">Decisions</h2>
        {#each threadsStore.currentThread.decisions as decision (decision.decision_id)}
          <DecisionCard {decision} />
        {/each}
      </div>
    {/if}

    <!-- Merlin Input -->
    <MerlinInput threadId={threadsStore.currentThread.thread_id} />
  </div>
{/if}
