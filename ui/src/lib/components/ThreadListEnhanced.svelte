<script lang="ts">
  import { threadsStore, loadThreads } from '$stores/threads.svelte';
  import { api } from '$lib/api';
  import type { Thread, ThreadStatus } from '$lib/types';

  interface Props {
    onSelectThread?: (threadId: string) => void;
  }

  let { onSelectThread }: Props = $props();

  // Filters
  let statusFilter = $state<ThreadStatus | 'all'>('all');
  let participantFilter = $state<string>('all');
  let searchQuery = $state<string>('');
  let dateFilter = $state<'all' | 'today' | 'week' | 'month'>('all');

  // Access reactive state directly from the store
  let threads = $derived(threadsStore.threads);
  let loading = $derived(threadsStore.loading);
  let error = $derived(threadsStore.error);

  // Unique participants from threads
  let participants = $derived(() => {
    const uniqueAgents = new Set<string>();
    threads.forEach(thread => {
      thread.participants.forEach(p => uniqueAgents.add(p));
    });
    return Array.from(uniqueAgents).sort();
  });

  // Filtered threads
  let filteredThreads = $derived(() => {
    return threads.filter(thread => {
      // Status filter
      if (statusFilter !== 'all' && thread.status !== statusFilter) {
        return false;
      }

      // Participant filter
      if (participantFilter !== 'all' && !thread.participants.includes(participantFilter)) {
        return false;
      }

      // Search filter
      if (searchQuery && searchQuery.trim() !== '') {
        const query = searchQuery.toLowerCase();
        const subjectMatch = thread.subject?.toLowerCase().includes(query);
        if (!subjectMatch) {
          return false;
        }
      }

      // Date filter
      if (dateFilter !== 'all') {
        const threadDate = new Date(thread.created_at);
        const now = new Date();
        const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
        const weekAgo = new Date(today.getTime() - 7 * 24 * 60 * 60 * 1000);
        const monthAgo = new Date(today.getTime() - 30 * 24 * 60 * 60 * 1000);

        switch (dateFilter) {
          case 'today':
            if (threadDate < today) return false;
            break;
          case 'week':
            if (threadDate < weekAgo) return false;
            break;
          case 'month':
            if (threadDate < monthAgo) return false;
            break;
        }
      }

      return true;
    });
  });

  // Statistics
  let stats = $derived({
    total: threads.length,
    active: threads.filter(t => t.status === 'active').length,
    paused: threads.filter(t => t.status === 'paused').length,
    resolved: threads.filter(t => t.status === 'resolved').length,
    archived: threads.filter(t => t.status === 'archived').length,
    totalUnread: threads.reduce((sum, t) => sum + (t.unread_count || 0), 0),
  });

  async function handleStatusChange(newStatus: typeof statusFilter) {
    statusFilter = newStatus;
    await loadThreads(newStatus === 'all' ? 'all' : newStatus);
  }

  async function refreshThreads() {
    await loadThreads(statusFilter === 'all' ? 'all' : statusFilter);
  }

  function clearFilters() {
    statusFilter = 'all';
    participantFilter = 'all';
    searchQuery = '';
    dateFilter = 'all';
  }

  function getStatusBadgeClass(status: ThreadStatus): string {
    const base = 'px-2 py-1 rounded-full text-xs font-medium ';
    switch (status) {
      case 'active':
        return base + 'bg-green-100 text-green-800 border border-green-200';
      case 'paused':
        return base + 'bg-yellow-100 text-yellow-800 border border-yellow-200';
      case 'resolved':
        return base + 'bg-blue-100 text-blue-800 border border-blue-200';
      case 'archived':
        return base + 'bg-gray-100 text-gray-800 border border-gray-200';
      default:
        return base + 'bg-gray-100 text-gray-800';
    }
  }

  function getStatusIcon(status: ThreadStatus): string {
    switch (status) {
      case 'active':
        return '🟢';
      case 'paused':
        return '⏸️';
      case 'resolved':
        return '✅';
      case 'archived':
        return '📦';
      default:
        return '❓';
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
    return date.toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
    });
  }

  function getThreadPreview(thread: Thread): string {
    // Get first 100 chars of subject
    if (!thread.subject) return 'No subject';
    return thread.subject.length > 100
      ? thread.subject.substring(0, 100) + '...'
      : thread.subject;
  }
</script>

<div class="thread-list-enhanced-container">
<div class="thread-list-enhanced">
  <!-- Header -->
  <div class="list-header">
    <div class="header-left">
      <h2>Threads</h2>
      <div class="stats">
        <span class="stat-badge">{stats.total} total</span>
        <span class="stat-badge active">{stats.active} active</span>
        {#if stats.totalUnread > 0}
          <span class="stat-badge unread">{stats.totalUnread} unread</span>
        {/if}
      </div>
    </div>

    <button
      class="refresh-btn"
      onclick={refreshThreads}
      disabled={loading}
      title="Refresh threads"
    >
      {loading ? '⏳' : '🔄'}
    </button>
  </div>

  <!-- Filters -->
  <div class="filters-panel">
    <div class="filter-row">
      <!-- Status Filter -->
      <div class="filter-group">
        <label for="status-filter">Status:</label>
        <select id="status-filter" bind:value={statusFilter}>
          <option value="all">All</option>
          <option value="active">🟢 Active</option>
          <option value="paused">⏸️ Paused</option>
          <option value="resolved">✅ Resolved</option>
          <option value="archived">📦 Archived</option>
        </select>
      </div>

      <!-- Participant Filter -->
      <div class="filter-group">
        <label for="participant-filter">Participant:</label>
        <select id="participant-filter" bind:value={participantFilter}>
          <option value="all">All Agents</option>
          {#each participants as agent}
            <option value={agent}>{agent}</option>
          {/each}
        </select>
      </div>

      <!-- Date Filter -->
      <div class="filter-group">
        <label for="date-filter">Date:</label>
        <select id="date-filter" bind:value={dateFilter}>
          <option value="all">All Time</option>
          <option value="today">Today</option>
          <option value="week">This Week</option>
          <option value="month">This Month</option>
        </select>
      </div>

      <!-- Search -->
      <div class="filter-group flex-1">
        <label for="search-input">Search:</label>
        <input
          id="search-input"
          type="text"
          placeholder="Search subjects..."
          bind:value={searchQuery}
        />
      </div>

      <!-- Clear Filters -->
      <button
        class="clear-btn"
        onclick={clearFilters}
        title="Clear all filters"
      >
        🗑️ Clear
      </button>
    </div>
  </div>

  <!-- Error State -->
  {#if error}
    <div class="error-state">
      <div class="error-icon">⚠️</div>
      <p class="error-message">{error}</p>
      <button class="retry-btn" onclick={refreshThreads}>Retry</button>
    </div>
  {/if}

  <!-- Loading State -->
  {#if loading && threads.length === 0}
    <div class="loading-state">
      <div class="loading-spinner"></div>
      <p>Loading threads...</p>
    </div>
  {/if}

  <!-- Empty State -->
  {#if !loading && filteredThreads.length === 0 && threads.length === 0}
    <div class="empty-state">
      <div class="empty-icon">🧵</div>
      <p>No threads found</p>
      <p class="empty-hint">Threads will appear here when agents communicate</p>
    </div>
  {:else if !loading && filteredThreads.length === 0}
    <div class="empty-state">
      <div class="empty-icon">🔍</div>
      <p>No threads match your filters</p>
      <button class="clear-filters-btn" onclick={clearFilters}>
        Clear Filters
      </button>
    </div>
  {/if}

  <!-- Thread List -->
  <div class="thread-list">
    {#each filteredThreads as thread (thread.id)}
      <div
        class="thread-card"
        class:has-unread={(thread.unread_count || 0) > 0}
        onclick={() => onSelectThread?.(thread.id)}
        role="button"
        tabindex="0"
        onkeydown={(e) => e.key === 'Enter' && onSelectThread?.(thread.id)}
      >
        <!-- Thread Header -->
        <div class="thread-header">
          <div class="thread-title">
            <span class="status-icon">{getStatusIcon(thread.status)}</span>
            <h3 class="thread-subject">{getThreadPreview(thread)}</h3>
            {#if (thread.unread_count || 0) > 0}
              <span class="unread-badge">{thread.unread_count} new</span>
            {/if}
          </div>

          <div class="thread-meta">
            <span class={getStatusBadgeClass(thread.status)}>{thread.status}</span>
            <span class="thread-date">{formatDate(thread.created_at)}</span>
          </div>
        </div>

        <!-- Thread Details -->
        <div class="thread-details">
          <!-- Participants -->
          <div class="detail-group">
            <span class="detail-label">👥</span>
            <span class="detail-value">{thread.participants.join(', ')}</span>
          </div>

          <!-- Message Count -->
          <div class="detail-group">
            <span class="detail-label">💬</span>
            <span class="detail-value">{thread.message_count} messages</span>
          </div>

          <!-- Thread ID -->
          <div class="detail-group monospace">
            <span class="detail-label">ID:</span>
            <span class="detail-value">{thread.id.slice(0, 8)}...</span>
          </div>
        </div>

        <!-- Thread Footer -->
        <div class="thread-footer">
          <span class="footer-text">
            Created {formatDate(thread.created_at)}
          </span>
        </div>
      </div>
    {/each}
  </div>

  <!-- List Footer -->
  {#if filteredThreads.length > 0}
    <div class="list-footer">
      <span class="result-count">
        Showing {filteredThreads.length} of {stats.total} threads
      </span>
    </div>
  {/if}
</div>
</div>

<style>
  .thread-list-enhanced-container {
    max-height: calc(100vh - 12rem);
    overflow-y: auto;
  }

  .thread-list-enhanced {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  /* Header */
  .list-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1.5rem;
    background: white;
    border-radius: 8px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
  }

  .header-left {
    display: flex;
    align-items: baseline;
    gap: 1rem;
  }

  .header-left h2 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
    color: #111827;
  }

  .stats {
    display: flex;
    gap: 0.5rem;
  }

  .stat-badge {
    padding: 0.25rem 0.75rem;
    border-radius: 9999px;
    font-size: 0.875rem;
    font-weight: 500;
    background: #f3f4f6;
    color: #374151;
  }

  .stat-badge.active {
    background: #d1fae5;
    color: #065f46;
  }

  .stat-badge.unread {
    background: #fee2e2;
    color: #991b1b;
    font-weight: 600;
  }

  .refresh-btn {
    padding: 0.5rem 1rem;
    border: 1px solid #d1d5db;
    background: white;
    border-radius: 6px;
    cursor: pointer;
    font-size: 1rem;
    transition: all 0.2s;
  }

  .refresh-btn:hover:not(:disabled) {
    background: #f3f4f6;
    border-color: #9ca3af;
  }

  .refresh-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* Filters */
  .filters-panel {
    padding: 1rem 1.5rem;
    background: white;
    border-radius: 8px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
  }

  .filter-row {
    display: flex;
    gap: 1rem;
    align-items: center;
    flex-wrap: wrap;
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
  }

  .filter-group select {
    min-width: 120px;
  }

  .filter-group input {
    width: 250px;
  }

  .filter-group input:focus,
  .filter-group select:focus {
    outline: none;
    border-color: #3b82f6;
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.1);
  }

  .clear-btn {
    padding: 0.5rem 1rem;
    border: 1px solid #d1d5db;
    background: white;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.875rem;
    transition: all 0.2s;
  }

  .clear-btn:hover {
    background: #fef2f2;
    border-color: #ef4444;
    color: #ef4444;
  }

  /* States */
  .error-state,
  .loading-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 3rem;
    background: white;
    border-radius: 8px;
    text-align: center;
  }

  .error-icon,
  .empty-icon {
    font-size: 4rem;
    margin-bottom: 1rem;
  }

  .error-message {
    color: #ef4444;
    margin-bottom: 1rem;
  }

  .retry-btn,
  .clear-filters-btn {
    padding: 0.5rem 1rem;
    background: #3b82f6;
    color: white;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.875rem;
  }

  .loading-spinner {
    width: 40px;
    height: 40px;
    border: 3px solid #e5e7eb;
    border-top-color: #3b82f6;
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .empty-hint {
    color: #9ca3af;
    font-size: 0.875rem;
  }

  /* Thread List */
  .thread-list {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .thread-card {
    background: white;
    border: 2px solid #e5e7eb;
    border-radius: 8px;
    padding: 1rem 1.25rem;
    cursor: pointer;
    transition: all 0.2s;
  }

  .thread-card:hover {
    border-color: #3b82f6;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.1);
  }

  .thread-card:focus {
    outline: none;
    border-color: #3b82f6;
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.1);
  }

  .thread-card.has-unread {
    border-left: 4px solid #ef4444;
  }

  .thread-header {
    display: flex;
    justify-content: space-between;
    align-items: start;
    margin-bottom: 0.75rem;
  }

  .thread-title {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex: 1;
    min-width: 0;
  }

  .status-icon {
    font-size: 1.25rem;
  }

  .thread-subject {
    margin: 0;
    font-size: 1rem;
    font-weight: 600;
    color: #111827;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .unread-badge {
    padding: 0.25rem 0.5rem;
    background: #ef4444;
    color: white;
    font-size: 0.75rem;
    font-weight: 600;
    border-radius: 9999px;
    flex-shrink: 0;
  }

  .thread-meta {
    display: flex;
    flex-direction: column;
    align-items: end;
    gap: 0.25rem;
  }

  .thread-date {
    font-size: 0.75rem;
    color: #9ca3af;
    font-family: 'SF Mono', Monaco, monospace;
  }

  /* Thread Details */
  .thread-details {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
    padding: 0.75rem 0;
    border-top: 1px solid #f3f4f6;
    border-bottom: 1px solid #f3f4f6;
  }

  .detail-group {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.875rem;
  }

  .detail-label {
    color: #6b7280;
  }

  .detail-value {
    color: #374151;
    font-weight: 500;
  }

  .monospace {
    font-family: 'SF Mono', Monaco, monospace;
  }

  /* Thread Footer */
  .thread-footer {
    padding-top: 0.5rem;
  }

  .footer-text {
    font-size: 0.75rem;
    color: #9ca3af;
  }

  /* List Footer */
  .list-footer {
    padding: 0.75rem 1.5rem;
    background: white;
    border-radius: 8px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
  }

  .result-count {
    font-size: 0.875rem;
    color: #6b7280;
  }
</style>
