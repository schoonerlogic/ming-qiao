<script lang="ts">
  // Disable SSR for this page to prevent hydration errors with $state runes
  export const ssr = false;

  import ThreadList from '$lib/components/ThreadList.svelte';
  import ThreadView from '$lib/components/ThreadView.svelte';
  import SearchBar from '$lib/components/SearchBar.svelte';
  import ModeToggle from '$lib/components/ModeToggle.svelte';
  import NotificationCenter from '$lib/components/NotificationCenter.svelte';
  import MerlinNotificationStream from '$lib/components/MerlinNotificationStream.svelte';
  import { loadThreads, loadThread, clearCurrentThread } from '$stores/threads';
  import { loadConfig } from '$stores/config';
  import { connect, onMessage } from '$stores/websocket';
  import { onMount } from 'svelte';

  // View state: 'list' or 'detail'
  let currentView = $state<'list' | 'detail'>('list');
  let wsConnected = $state(false);

  onMount(() => {
    const init = async () => {
      // Load initial data
      console.log('[DEBUG] Loading config...');
      try {
        await loadConfig();
        console.log('[DEBUG] Config loaded successfully');
      } catch (e) {
        console.error('[DEBUG] Config load failed:', e);
      }

      console.log('[DEBUG] Loading threads...');
      try {
        await loadThreads('active');
        console.log('[DEBUG] Threads loaded successfully');
      } catch (e) {
        console.error('[DEBUG] Thread load failed:', e);
      }
    };

    // Initialize data
    init();

    // Connect WebSocket
    console.log('[DEBUG] Connecting WebSocket...');
    try {
      connect();
      console.log('[DEBUG] WebSocket connect() called');
    } catch (e) {
      console.error('[DEBUG] WebSocket connect failed:', e);
    }

    // Subscribe to WebSocket messages
    const unsubscribe = onMessage((message) => {
      console.log('WebSocket message:', message);
      
      switch (message.type) {
        case 'connected':
          console.log('[DEBUG] WebSocket connected message received');
          wsConnected = true;
          break;
        case 'message':
          // Refresh threads when new message arrives
          loadThreads('active');
          break;
        case 'thread_status':
          // Refresh thread list when status changes
          loadThreads('active');
          break;
        case 'mode_changed':
          // Reload config when mode changes
          loadConfig();
          break;
      }
    });

    // Return cleanup function
    return unsubscribe;
  });

  function selectThread(threadId: string) {
    loadThread(threadId);
    currentView = 'detail';
  }

  function backToList() {
    clearCurrentThread();
    currentView = 'list';
  }
</script>

<div class="min-h-screen bg-gray-50">
  <!-- Header -->
  <header class="bg-white border-b border-gray-200 sticky top-0 z-10">
    <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
      <div class="flex items-center justify-between h-16">
        <div class="flex items-center gap-4">
          <button
            class="p-2 hover:bg-gray-100 rounded-md transition-colors"
            class:hidden={!currentView || currentView === 'list'}
            onclick={backToList}
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path
                stroke-linecap="round"
                stroke-linejoin="round"
                stroke-width="2"
                d="M15 19l-7-7 7-7"
              />
            </svg>
          </button>
          <h1 class="text-xl font-bold text-gray-900">Ming-Qiao</h1>
          <span class="text-sm text-gray-500">Council of Wizards</span>
        </div>

        <div class="flex items-center gap-4">
          <!-- Merlin Notification Center -->
          <NotificationCenter />

          <div class="flex items-center gap-2">
            <div class="w-2 h-2 rounded-full {wsConnected
              ? 'bg-green-500'
              : 'bg-gray-400'}"></div>
            <span class="text-xs text-gray-600">
              {wsConnected ? 'Connected' : 'Disconnected'}
            </span>
          </div>
        </div>
      </div>
    </div>
  </header>

  <!-- Main Content -->
  <main class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6">
    <!-- Search and Mode Bar -->
    <div class="mb-6 flex gap-4">
      <div class="flex-1">
        <SearchBar />
      </div>
      <div class="w-80">
        <ModeToggle />
      </div>
    </div>

    <!-- View: Thread List -->
    {#if currentView === 'list'}
      <div class="thread-list-view">
        <ThreadList 
          onSelectThread={(threadId) => selectThread(threadId)}
        />
      </div>
    {:else}
      <div class="thread-detail-view">
        <ThreadView />
      </div>
    {/if}
  </main>

  <!-- Footer -->
  <footer class="bg-white border-t border-gray-200 mt-12">
    <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-4">
      <div class="flex items-center justify-between text-sm text-gray-500">
        <p>Ming-Qiao v0.1.0 — AstralMaris subsystem</p>
        <p>Built with SvelteKit + Svelte 5</p>
      </div>
    </div>
  </footer>
</div>

<!-- Merlin Notification Stream (invisible, manages WebSocket connection) -->
<MerlinNotificationStream />

<style>
  :global(html) {
    scroll-behavior: smooth;
  }
</style>
