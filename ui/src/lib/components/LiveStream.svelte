<script lang="ts">
  import { onMount } from 'svelte';
  import { connect, onMessage, disconnect, wsStore, type WSMessage } from '$stores/websocket.svelte';
  import type { EventEnvelope } from '$lib/types';

  // Stellar-chroma inspired color palette for agents
  const agentColors: Record<string, string> = {
    merlin: '#8B5CF6', // Violet
    proteus: '#0EA5E9', // Sky blue
    thales: '#F59E0B', // Amber
    aleph: '#10B981', // Emerald
    luban: '#EF4444', // Red
    'laozi-jung': '#6366F1', // Indigo
    unknown: '#6B7280', // Gray
  };

  // State
  let events = $state<EventEnvelope[]>([]);
  let filteredEvents = $state<EventEnvelope[]>([]);
  let autoScroll = $state(true);
  let isPaused = $state(false);

  // Filters
  let filterAgent = $state<string>('all');
  let filterEventType = $state<string>('all');
  let filterSubject = $state<string>('');
  let eventTypes = $state<string[]>([]);

  // Connection state
  let wsConnected = $derived(wsStore.connected);
  let streamContainer: HTMLElement;

  onMount(() => {
    // Connect to WebSocket
    connect();

    // Subscribe to WebSocket messages
    const unsubscribe = onMessage((message: WSMessage) => {
      if (message.type === 'connected') {
        console.log('Connected to Merlin notifications:', message);
      } else if (message.type === 'priority_alert' ||
                 message.type === 'keyword_detected' ||
                 message.type === 'decision_review' ||
                 message.type === 'action_blocked') {
        // Add event to stream
        addEvent(message.event);
      } else if (message.type === 'status_update') {
        console.log('Status update:', message.message);
      } else if (message.type === 'error') {
        console.error('WebSocket error:', message.message);
      }
    });

    // Cleanup on unmount
    return () => {
      unsubscribe();
      disconnect();
    };
  });

  function addEvent(event: EventEnvelope) {
    if (isPaused) return;

    // Add to beginning of array (newest first)
    events = [event, ...events];

    // Update unique event types
    if (!eventTypes.includes(event.event_type)) {
      eventTypes = [...eventTypes, event.event_type];
    }

    // Apply filters
    applyFilters();

    // Auto-scroll if enabled
    if (autoScroll && streamContainer) {
      setTimeout(() => {
        streamContainer.scrollTop = 0;
      }, 10);
    }
  }

  function applyFilters() {
    filteredEvents = events.filter(event => {
      // Agent filter
      if (filterAgent !== 'all' && event.agent_id !== filterAgent) {
        return false;
      }

      // Event type filter
      if (filterEventType !== 'all' && event.event_type !== filterEventType) {
        return false;
      }

      // Subject filter (if set)
      if (filterSubject && filterSubject.trim() !== '') {
        const searchLower = filterSubject.toLowerCase();
        const subjectMatch = event.payload?.subject?.toLowerCase().includes(searchLower);
        const contentMatch = event.payload?.content?.toLowerCase().includes(searchLower);
        if (!subjectMatch && !contentMatch) {
          return false;
        }
      }

      return true;
    });
  }

  function getAgentColor(agentId: string): string {
    return agentColors[agentId] || agentColors.unknown;
  }

  function getAgentInitials(agentId: string): string {
    return agentId
      .split('-')
      .map(part => part.charAt(0).toUpperCase())
      .join('')
      .substring(0, 2);
  }

  function formatTimestamp(timestamp: string): string {
    const date = new Date(timestamp);
    return date.toLocaleTimeString('en-US', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false
    });
  }

  function getEventIcon(eventType: string): string {
    switch (eventType.toLowerCase()) {
      case 'message_sent':
        return '💬';
      case 'decision_recorded':
        return '🎯';
      case 'thread_created':
        return '🧵';
      case 'artifact_shared':
        return '📦';
      case 'agent_status':
        return '🔄';
      default:
        return '📡';
    }
  }

  function clearStream() {
    events = [];
    filteredEvents = [];
  }

  function togglePause() {
    isPaused = !isPaused;
  }

  // Reactive filter updates
  $effect(() => {
    applyFilters();
  });
</script>

<div class="live-stream">
  <!-- Header -->
  <div class="stream-header">
    <div class="header-title">
      <h2>Live Agent Stream</h2>
      <div class="connection-status">
        <span class="status-dot {wsConnected ? 'connected' : 'disconnected'}"></span>
        <span class="status-text">{wsConnected ? 'Connected' : 'Disconnected'}</span>
      </div>
    </div>

    <div class="header-actions">
      <button
        class="action-btn"
        onclick={togglePause}
        title={isPaused ? 'Resume stream' : 'Pause stream'}
      >
        {isPaused ? '▶️ Resume' : '⏸️ Pause'}
      </button>

      <button
        class="action-btn"
        onclick={() => autoScroll = !autoScroll}
        title={autoScroll ? 'Disable auto-scroll' : 'Enable auto-scroll'}
      >
        {autoScroll ? '🔝 Auto-scroll' : '📜 Manual scroll'}
      </button>

      <button
        class="action-btn danger"
        onclick={clearStream}
        title="Clear stream"
      >
        🗑️ Clear
      </button>
    </div>
  </div>

  <!-- Filters -->
  <div class="stream-filters">
    <div class="filter-group">
      <label for="agent-filter">Agent:</label>
      <select id="agent-filter" bind:value={filterAgent}>
        <option value="all">All Agents</option>
        <option value="merlin">Merlin</option>
        <option value="proteus">Proteus</option>
        <option value="thales">Thales</option>
        <option value="aleph">Aleph</option>
        <option value="luban">Luban</option>
        <option value="laozi-jung">Laozi-Jung</option>
      </select>
    </div>

    <div class="filter-group">
      <label for="type-filter">Event Type:</label>
      <select id="type-filter" bind:value={filterEventType}>
        <option value="all">All Events</option>
        {#each eventTypes as type}
          <option value={type}>{type}</option>
        {/each}
      </select>
    </div>

    <div class="filter-group">
      <label for="subject-filter">Search:</label>
      <input
        id="subject-filter"
        type="text"
        placeholder="Subject or content..."
        bind:value={filterSubject}
      />
    </div>
  </div>

  <!-- Event Stream -->
  <div class="stream-events" bind:this={streamContainer}>
    {#if filteredEvents.length === 0}
      <div class="empty-state">
        <div class="empty-icon">📡</div>
        <p>Waiting for events...</p>
        {#if isPaused}
          <p class="paused-text">Stream is paused</p>
        {/if}
      </div>
    {:else}
      {#each filteredEvents as event (event.event_id)}
        <div class="event-card">
          <!-- Event Header -->
          <div class="event-header">
            <div class="event-meta">
              <span class="event-icon">{getEventIcon(event.event_type)}</span>
              <span class="event-time">{formatTimestamp(event.timestamp)}</span>
              <span class="event-type">{event.event_type}</span>
            </div>

            <div class="agent-badge" style="background-color: {getAgentColor(event.agent_id)}20; border-left: 3px solid {getAgentColor(event.agent_id)}">
              <span class="agent-initials" style="color: {getAgentColor(event.agent_id)}">
                {getAgentInitials(event.agent_id)}
              </span>
              <span class="agent-name">{event.agent_id}</span>
            </div>
          </div>

          <!-- Event Content -->
          {#if event.payload?.subject || event.payload?.content}
            <div class="event-content">
              {#if event.payload?.subject}
                <div class="event-subject">{event.payload.subject}</div>
              {/if}
              {#if event.payload?.content}
                <div class="event-payload">{event.payload.content}</div>
              {/if}
            </div>
          {/if}

          <!-- Event Details -->
          {#if event.payload?.to_agent}
            <div class="event-details">
              <span class="detail-label">To:</span>
              <span class="detail-value">{event.payload.to_agent}</span>
            </div>
          {/if}

          {#if event.payload?.thread_id}
            <div class="event-details">
              <span class="detail-label">Thread:</span>
              <span class="detail-value monospace">{event.payload.thread_id}</span>
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </div>

  <!-- Stream Footer -->
  <div class="stream-footer">
    <span class="event-count">
      {filteredEvents.length} {filteredEvents.length === 1 ? 'event' : 'events'}
    </span>
    {#if isPaused}
      <span class="paused-indicator">⏸️ Paused</span>
    {/if}
  </div>
</div>

<style>
  .live-stream {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: white;
    border-radius: 8px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
    overflow: hidden;
  }

  /* Header */
  .stream-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1rem 1.5rem;
    border-bottom: 1px solid #e5e7eb;
    background: #f9fafb;
  }

  .header-title {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  .header-title h2 {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 600;
    color: #111827;
  }

  .connection-status {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.875rem;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .status-dot.connected {
    background-color: #10b981;
    box-shadow: 0 0 8px rgba(16, 185, 129, 0.4);
  }

  .status-dot.disconnected {
    background-color: #6b7280;
  }

  .status-text {
    color: #6b7280;
  }

  .header-actions {
    display: flex;
    gap: 0.5rem;
  }

  .action-btn {
    padding: 0.5rem 1rem;
    font-size: 0.875rem;
    border: 1px solid #d1d5db;
    background: white;
    border-radius: 6px;
    cursor: pointer;
    transition: all 0.2s;
  }

  .action-btn:hover {
    background: #f3f4f6;
    border-color: #9ca3af;
  }

  .action-btn.danger:hover {
    background: #fef2f2;
    border-color: #ef4444;
    color: #ef4444;
  }

  /* Filters */
  .stream-filters {
    display: flex;
    gap: 1rem;
    padding: 1rem 1.5rem;
    border-bottom: 1px solid #e5e7eb;
    background: #ffffff;
  }

  .filter-group {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .filter-group label {
    font-size: 0.875rem;
    font-weight: 500;
    color: #374151;
    white-space: nowrap;
  }

  .filter-group select,
  .filter-group input {
    padding: 0.5rem;
    border: 1px solid #d1d5db;
    border-radius: 6px;
    font-size: 0.875rem;
    min-width: 150px;
  }

  .filter-group input {
    width: 200px;
  }

  .filter-group select:focus,
  .filter-group input:focus {
    outline: none;
    border-color: #3b82f6;
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.1);
  }

  /* Event Stream */
  .stream-events {
    flex: 1;
    overflow-y: auto;
    padding: 1rem;
    background: #f9fafb;
    scroll-behavior: smooth;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: #9ca3af;
    text-align: center;
  }

  .empty-icon {
    font-size: 4rem;
    margin-bottom: 1rem;
    opacity: 0.5;
  }

  .paused-text {
    font-size: 0.875rem;
    color: #f59e0b;
    margin-top: 0.5rem;
  }

  .event-card {
    background: white;
    border: 1px solid #e5e7eb;
    border-radius: 8px;
    padding: 1rem;
    margin-bottom: 0.75rem;
    transition: all 0.2s;
  }

  .event-card:hover {
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
    border-color: #d1d5db;
  }

  .event-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.75rem;
  }

  .event-meta {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .event-icon {
    font-size: 1.25rem;
  }

  .event-time {
    font-size: 0.75rem;
    color: #9ca3af;
    font-family: 'SF Mono', Monaco, monospace;
  }

  .event-type {
    font-size: 0.75rem;
    padding: 0.25rem 0.5rem;
    background: #f3f4f6;
    border-radius: 4px;
    color: #6b7280;
    font-weight: 500;
  }

  .agent-badge {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.375rem 0.75rem;
    border-radius: 6px;
  }

  .agent-initials {
    font-weight: 600;
    font-size: 0.75rem;
  }

  .agent-name {
    font-size: 0.875rem;
    font-weight: 500;
    color: #374151;
  }

  .event-content {
    margin-bottom: 0.75rem;
  }

  .event-subject {
    font-weight: 600;
    color: #111827;
    margin-bottom: 0.25rem;
  }

  .event-payload {
    font-size: 0.875rem;
    color: #4b5563;
    line-height: 1.5;
    white-space: pre-wrap;
  }

  .event-details {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.75rem;
    color: #6b7280;
  }

  .detail-label {
    font-weight: 500;
  }

  .detail-value {
    color: #374151;
  }

  .monospace {
    font-family: 'SF Mono', Monaco, monospace;
  }

  /* Footer */
  .stream-footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.75rem 1.5rem;
    border-top: 1px solid #e5e7eb;
    background: #f9fafb;
    font-size: 0.875rem;
  }

  .event-count {
    color: #6b7280;
  }

  .paused-indicator {
    color: #f59e0b;
    font-weight: 500;
  }
</style>
