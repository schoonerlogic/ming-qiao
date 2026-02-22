<script lang="ts">
  import ThreadListEnhanced from '$lib/components/ThreadListEnhanced.svelte';
  import ThreadView from '$lib/components/ThreadView.svelte';
  import LiveStream from '$lib/components/LiveStream.svelte';
  import DecisionLog from '$lib/components/DecisionLog.svelte';
  import InterventionPanel from '$lib/components/InterventionPanel.svelte';
  import AgentStatusPanel from '$lib/components/AgentStatusPanel.svelte';
  import SearchBar from '$lib/components/SearchBar.svelte';
  import ModeToggle from '$lib/components/ModeToggle.svelte';
  import { loadThreads, loadThread, clearCurrentThread } from '$stores/threads.svelte';
  import { loadConfig } from '$stores/config.svelte';
  import { connect, onMessage, wsStore } from '$stores/websocket.svelte';
  import { api } from '$lib/api';
  import { onMount } from 'svelte';
  import type { EventEnvelope } from '$lib/types';

  // View state: 'live', 'threads', 'decisions', 'intervene', 'status', or 'detail'
  type ViewMode = 'live' | 'threads' | 'decisions' | 'intervene' | 'status' | 'detail';
  let currentView = $state<ViewMode>('live');
  let wsConnected = $derived(wsStore.connected);

  // New Thread Dialog state
  let showNewThreadDialog = $state(false);
  let newThreadTo = $state<string[]>([]);
  let newThreadSubject = $state('');
  let newThreadContent = $state('');
  let newThreadError = $state('');
  let newThreadLoading = $state(false);

  const availableAgents = [
    { id: 'merlin', name: 'Merlin' },
    { id: 'aleph', name: 'Aleph' },
    { id: 'thales', name: 'Thales' },
    { id: 'luban', name: 'Luban' },
    { id: 'laozi-jung', name: 'Laozi-Jung' },
    { id: 'proteus', name: 'Proteus' },
  ];

  // Reference to AgentStatusPanel for updates
  let agentStatusPanel: AgentStatusPanel;

  onMount(() => {
    const init = async () => {
      // Load initial data
      console.log('[+page] Initializing data load...');
      await loadConfig();
      console.log('[+page] Config loaded');
      await loadThreads('active');
      console.log('[+page] Threads loaded');
    };

    // Initialize data
    init();

    // Subscribe to WebSocket messages (LiveStream connects on its own)
    const unsubscribe = onMessage((message) => {
      console.log('WebSocket message:', message);

      switch (message.type) {
        case 'connected':
          // Connection state is derived from wsStore
          break;
        case 'priority_alert':
        case 'keyword_detected':
          // Refresh threads when new message arrives
          loadThreads('active');
          // Update agent status from event
          if (agentStatusPanel && message.event) {
            agentStatusPanel.updateFromEvent(message.event);
          }
          break;
        case 'status_update':
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

  function switchView(view: ViewMode) {
    currentView = view;
  }

  async function handleCreateThread() {
    if (!newThreadTo.length || !newThreadSubject.trim() || !newThreadContent.trim()) {
      newThreadError = 'Please fill in all fields';
      return;
    }

    newThreadLoading = true;
    newThreadError = '';

    try {
      // Create thread with first selected agent
      const primaryAgent = newThreadTo[0];
      await api.createThread({
        from: 'merlin',
        to: primaryAgent,
        subject: newThreadSubject.trim(),
        content: newThreadContent.trim(),
        priority: 'normal'
      });

      // Reset form
      newThreadTo = [];
      newThreadSubject = '';
      newThreadContent = '';
      showNewThreadDialog = false;

      // Refresh threads
      await loadThreads('active');
    } catch (e) {
      newThreadError = e instanceof Error ? e.message : 'Failed to create thread';
    } finally {
      newThreadLoading = false;
    }
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
            class:hidden={!currentView || currentView === 'live' || currentView === 'threads' || currentView === 'decisions' || currentView === 'status'}
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
          <span class="text-sm text-gray-500">Merlin's Console</span>
        </div>

        <!-- Navigation Tabs -->
        <nav class="flex items-center gap-2">
          <button
            class="px-4 py-2 rounded-md text-sm font-medium transition-colors {currentView === 'live'
              ? 'bg-blue-50 text-blue-700'
              : 'text-gray-600 hover:bg-gray-100'}"
            onclick={() => switchView('live')}
          >
            📡 Live
          </button>
          <button
            class="px-4 py-2 rounded-md text-sm font-medium transition-colors {currentView === 'threads'
              ? 'bg-blue-50 text-blue-700'
              : 'text-gray-600 hover:bg-gray-100'}"
            onclick={() => switchView('threads')}
          >
            🧵 Threads
          </button>
          <button
            class="px-4 py-2 rounded-md text-sm font-medium transition-colors {currentView === 'decisions'
              ? 'bg-blue-50 text-blue-700'
              : 'text-gray-600 hover:bg-gray-100'}"
            onclick={() => switchView('decisions')}
          >
            🎯 Decisions
          </button>
          <button
            class="px-4 py-2 rounded-md text-sm font-medium transition-colors {currentView === 'intervene'
              ? 'bg-blue-50 text-blue-700'
              : 'text-gray-600 hover:bg-gray-100'}"
            onclick={() => switchView('intervene')}
          >
            ⚡ Intervene
          </button>
          <button
            class="px-4 py-2 rounded-md text-sm font-medium transition-colors {currentView === 'status'
              ? 'bg-blue-50 text-blue-700'
              : 'text-gray-600 hover:bg-gray-100'}"
            onclick={() => switchView('status')}
          >
            👥 Agents
          </button>
        </nav>

        <div class="flex items-center gap-4">
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
  <main class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6" style="height: calc(100vh - 8rem);">
    <!-- View: Live Stream -->
    {#if currentView === 'live'}
      <div class="live-stream-view" style="height: calc(100vh - 10rem);">
        <LiveStream />
      </div>

    <!-- View: Thread List -->
    {:else if currentView === 'threads'}
      <div class="thread-list-view">
        <div class="mb-6 flex gap-4 items-center">
          <button
            class="px-4 py-2 bg-blue-500 hover:bg-blue-600 text-white rounded-md font-medium transition-colors"
            onclick={() => showNewThreadDialog = true}
          >
            ✉️ New Thread
          </button>
          <div class="flex-1">
            <SearchBar />
          </div>
          <div class="w-80">
            <ModeToggle />
          </div>
        </div>

        <ThreadListEnhanced
          onSelectThread={(threadId) => selectThread(threadId)}
        />
      </div>

    <!-- View: Decision Log -->
    {:else if currentView === 'decisions'}
      <div class="decision-log-view" style="height: calc(100vh - 10rem);">
        <DecisionLog />
      </div>

    <!-- View: Intervention Panel -->
    {:else if currentView === 'intervene'}
      <div class="intervention-view" style="height: calc(100vh - 10rem);">
        <InterventionPanel />
      </div>

    <!-- View: Agent Status -->
    {:else if currentView === 'status'}
      <div class="agent-status-view" style="height: calc(100vh - 10rem);">
        <AgentStatusPanel bind:this={agentStatusPanel} />
      </div>

    <!-- View: Thread Detail -->
    {:else}
      <div class="thread-detail-view">
        <ThreadView />
      </div>
    {/if}
  </main>

  <!-- New Thread Dialog -->
  {#if showNewThreadDialog}
    <div class="dialog-overlay" onclick={() => showNewThreadDialog = false}>
      <div class="dialog-content" onclick={(e) => e.stopPropagation()}>
        <div class="dialog-header">
          <h2>New Thread</h2>
          <button class="dialog-close" onclick={() => showNewThreadDialog = false}>✕</button>
        </div>

        <div class="dialog-body">
          {#if newThreadError}
            <div class="dialog-error">{newThreadError}</div>
          {/if}

          <div class="form-group">
            <label for="thread-to">To:</label>
            <select
              id="thread-to"
              multiple
              bind:value={newThreadTo}
              disabled={newThreadLoading}
            >
              {#each availableAgents as agent}
                <option value={agent.id}>{agent.name}</option>
              {/each}
            </select>
            <small class="form-hint">Hold Ctrl/Cmd to select multiple agents</small>
          </div>

          <div class="form-group">
            <label for="thread-subject">Subject:</label>
            <input
              id="thread-subject"
              type="text"
              bind:value={newThreadSubject}
              disabled={newThreadLoading}
              placeholder="Enter thread subject..."
            />
          </div>

          <div class="form-group">
            <label for="thread-content">Message:</label>
            <textarea
              id="thread-content"
              bind:value={newThreadContent}
              disabled={newThreadLoading}
              placeholder="Enter your message..."
              rows="4"
            ></textarea>
          </div>
        </div>

        <div class="dialog-footer">
          <button
            class="btn btn-secondary"
            onclick={() => showNewThreadDialog = false}
            disabled={newThreadLoading}
          >
            Cancel
          </button>
          <button
            class="btn btn-primary"
            onclick={handleCreateThread}
            disabled={newThreadLoading || !newThreadTo.length || !newThreadSubject.trim() || !newThreadContent.trim()}
          >
            {newThreadLoading ? 'Creating...' : 'Create Thread'}
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Footer -->
  <footer class="bg-white border-t border-gray-200 mt-12">
    <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-4">
      <div class="flex items-center justify-between text-sm text-gray-500">
        <p>Ming-Qiao v0.3.0 — AstralMaris subsystem</p>
        <p>Built with SvelteKit + Svelte 5</p>
      </div>
    </div>
  </footer>
</div>

<style>
  :global(html) {
    scroll-behavior: smooth;
  }

  /* Dialog Styles */
  .dialog-overlay {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .dialog-content {
    background: white;
    border-radius: 12px;
    box-shadow: 0 20px 25px -5px rgba(0, 0, 0, 0.1), 0 10px 10px -5px rgba(0, 0, 0, 0.04);
    max-width: 600px;
    width: 90%;
    max-height: 90vh;
    overflow-y: auto;
  }

  .dialog-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1.5rem;
    border-bottom: 1px solid #e5e7eb;
  }

  .dialog-header h2 {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 600;
    color: #111827;
  }

  .dialog-close {
    background: none;
    border: none;
    font-size: 1.5rem;
    cursor: pointer;
    color: #6b7280;
    padding: 0.25rem;
    border-radius: 4px;
  }

  .dialog-close:hover {
    background: #f3f4f6;
  }

  .dialog-body {
    padding: 1.5rem;
  }

  .dialog-error {
    background: #fee2e2;
    border: 1px solid #ef4444;
    color: #991b1b;
    padding: 0.75rem;
    border-radius: 6px;
    margin-bottom: 1rem;
    font-size: 0.875rem;
  }

  .form-group {
    margin-bottom: 1rem;
  }

  .form-group label {
    display: block;
    font-size: 0.875rem;
    font-weight: 500;
    color: #374151;
    margin-bottom: 0.5rem;
  }

  .form-group input,
  .form-group select,
  .form-group textarea {
    width: 100%;
    padding: 0.625rem;
    border: 1px solid #d1d5db;
    border-radius: 6px;
    font-size: 0.875rem;
    font-family: inherit;
  }

  .form-group input:focus,
  .form-group select:focus,
  .form-group textarea:focus {
    outline: none;
    border-color: #3b82f6;
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.1);
  }

  .form-group input:disabled,
  .form-group select:disabled,
  .form-group textarea:disabled {
    background: #f3f4f6;
    cursor: not-allowed;
  }

  .form-hint {
    display: block;
    font-size: 0.75rem;
    color: #6b7280;
    margin-top: 0.25rem;
  }

  .dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.75rem;
    padding: 1.5rem;
    border-top: 1px solid #e5e7eb;
  }

  .btn {
    padding: 0.625rem 1.25rem;
    border-radius: 6px;
    font-size: 0.875rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s;
    border: 1px solid transparent;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-secondary {
    background: white;
    border-color: #d1d5db;
    color: #374151;
  }

  .btn-secondary:hover:not(:disabled) {
    background: #f9fafb;
    border-color: #9ca3af;
  }

  .btn-primary {
    background: #3b82f6;
    color: white;
  }

  .btn-primary:hover:not(:disabled) {
    background: #2563eb;
  }
</style>
