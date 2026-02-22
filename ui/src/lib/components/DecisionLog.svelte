<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import type { Decision, DecisionStatus } from '$lib/types';

  // State
  let decisions = $state<Decision[]>([]);
  let filteredDecisions = $state<Decision[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);

  // Search and filters
  let searchQuery = $state<string>('');
  let statusFilter = $state<DecisionStatus | 'all'>('all');
  let threadFilter = $state<string>('all');

  // Thread IDs for filter
  let threadIds = $state<string[]>([]);

  // Statistics
  let stats = $derived({
    total: decisions.length,
    pending: decisions.filter(d => d.status === 'pending').length,
    approved: decisions.filter(d => d.status === 'approved').length,
    rejected: decisions.filter(d => d.status === 'rejected').length,
    superseded: decisions.filter(d => d.status === 'superseded').length,
  });

  onMount(() => {
    loadDecisions();
  });

  async function loadDecisions() {
    loading = true;
    error = null;

    try {
      const response = await api.getDecisions();
      decisions = response.decisions;

      // Extract unique thread IDs
      const uniqueThreads = [...new Set(decisions.map(d => d.thread_id))];
      threadIds = uniqueThreads;

      // Apply initial filters
      applyFilters();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load decisions';
      console.error('Error loading decisions:', e);
    } finally {
      loading = false;
    }
  }

  function applyFilters() {
    filteredDecisions = decisions.filter(decision => {
      // Status filter
      if (statusFilter !== 'all' && decision.status !== statusFilter) {
        return false;
      }

      // Thread filter
      if (threadFilter !== 'all' && decision.thread_id !== threadFilter) {
        return false;
      }

      // Search filter
      if (searchQuery && searchQuery.trim() !== '') {
        const query = searchQuery.toLowerCase();
        const questionMatch = decision.question?.toLowerCase().includes(query);
        const resolutionMatch = decision.resolution?.toLowerCase().includes(query);
        const rationaleMatch = decision.rationale?.toLowerCase().includes(query);

        if (!questionMatch && !resolutionMatch && !rationaleMatch) {
          return false;
        }
      }

      return true;
    });
  }

  // Reactive filter updates
  $effect(() => {
    applyFilters();
  });

  function getStatusColor(status: DecisionStatus): string {
    switch (status) {
      case 'pending':
        return '#f59e0b'; // Amber
      case 'approved':
        return '#10b981'; // Emerald
      case 'rejected':
        return '#ef4444'; // Red
      case 'superseded':
        return '#6b7280'; // Gray
      default:
        return '#9ca3af';
    }
  }

  function getStatusIcon(status: DecisionStatus): string {
    switch (status) {
      case 'pending':
        return '⏳';
      case 'approved':
        return '✅';
      case 'rejected':
        return '❌';
      case 'superseded':
        return '🔄';
      default:
        return '❓';
    }
  }

  function formatDate(dateStr: string): string {
    if (!dateStr) return 'N/A';
    const date = new Date(dateStr);
    return date.toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  function viewThread(threadId: string) {
    // Open thread in new tab or navigate
    window.open(`/thread/${threadId}`, '_blank');
  }

  async function approveDecision(decisionId: string) {
    try {
      await api.approveDecision(decisionId, 'Approved via Merlin console');
      await loadDecisions(); // Reload
    } catch (e) {
      console.error('Error approving decision:', e);
      alert('Failed to approve decision');
    }
  }

  async function rejectDecision(decisionId: string) {
    const reason = prompt('Reason for rejection:');
    if (!reason) return;

    try {
      await api.rejectDecision(decisionId, reason);
      await loadDecisions(); // Reload
    } catch (e) {
      console.error('Error rejecting decision:', e);
      alert('Failed to reject decision');
    }
  }
</script>

<div class="decision-log">
  <!-- Header -->
  <div class="log-header">
    <div class="header-title">
      <h2>Decision Log</h2>
      <div class="stats">
        <span class="stat-item pending">{stats.pending} Pending</span>
        <span class="stat-item approved">{stats.approved} Approved</span>
        <span class="stat-item rejected">{stats.rejected} Rejected</span>
        <span class="stat-item superseded">{stats.superseded} Superseded</span>
      </div>
    </div>

    <button class="refresh-btn" onclick={loadDecisions} disabled={loading}>
      {loading ? '⏳' : '🔄'} Refresh
    </button>
  </div>

  <!-- Filters -->
  <div class="log-filters">
    <div class="filter-group">
      <label for="search-input">Search:</label>
      <input
        id="search-input"
        type="text"
        placeholder="Question, resolution, rationale..."
        bind:value={searchQuery}
      />
    </div>

    <div class="filter-group">
      <label for="status-filter">Status:</label>
      <select id="status-filter" bind:value={statusFilter}>
        <option value="all">All Statuses</option>
        <option value="pending">⏳ Pending</option>
        <option value="approved">✅ Approved</option>
        <option value="rejected">❌ Rejected</option>
        <option value="superseded">🔄 Superseded</option>
      </select>
    </div>

    <div class="filter-group">
      <label for="thread-filter">Thread:</label>
      <select id="thread-filter" bind:value={threadFilter}>
        <option value="all">All Threads</option>
        {#each threadIds as threadId}
          <option value={threadId}>{threadId}</option>
        {/each}
      </select>
    </div>
  </div>

  <!-- Error State -->
  {#if error}
    <div class="error-state">
      <div class="error-icon">⚠️</div>
      <p class="error-message">{error}</p>
      <button class="retry-btn" onclick={loadDecisions}>Retry</button>
    </div>
  {/if}

  <!-- Loading State -->
  {#if loading && decisions.length === 0}
    <div class="loading-state">
      <div class="loading-spinner"></div>
      <p>Loading decisions...</p>
    </div>
  {/if}

  <!-- Empty State -->
  {#if !loading && filteredDecisions.length === 0 && decisions.length === 0}
    <div class="empty-state">
      <div class="empty-icon">🎯</div>
      <p>No decisions recorded yet</p>
      <p class="empty-hint">Decisions will appear here as agents make choices</p>
    </div>
  {:else if !loading && filteredDecisions.length === 0}
    <div class="empty-state">
      <div class="empty-icon">🔍</div>
      <p>No decisions match your filters</p>
      <button class="clear-filters-btn" onclick={() => { searchQuery = ''; statusFilter = 'all'; threadFilter = 'all'; }}>
        Clear Filters
      </button>
    </div>
  {/if}

  <!-- Decision List -->
  <div class="decision-list">
    {#each filteredDecisions as decision (decision.decision_id)}
      <div class="decision-card">
        <!-- Decision Header -->
        <div class="decision-header">
          <div class="decision-meta">
            <span class="decision-icon">{getStatusIcon(decision.status)}</span>
            <span class="decision-id monospace">{decision.decision_id}</span>
            <span class="decision-status" style="color: {getStatusColor(decision.status)}">
              {decision.status}
            </span>
          </div>

          <div class="decision-actions">
            {#if decision.status === 'pending'}
              <button
                class="action-btn approve"
                onclick={() => approveDecision(decision.decision_id)}
                title="Approve"
              >
                ✅ Approve
              </button>
              <button
                class="action-btn reject"
                onclick={() => rejectDecision(decision.decision_id)}
                title="Reject"
              >
                ❌ Reject
              </button>
            {/if}

            <button
              class="action-btn view"
              onclick={() => viewThread(decision.thread_id)}
              title="View thread"
            >
              🧵 View Thread
            </button>
          </div>
        </div>

        <!-- Decision Content -->
        <div class="decision-content">
          <h3 class="decision-question">{decision.question}</h3>

          {#if decision.resolution}
            <div class="decision-resolution">
              <span class="resolution-label">Resolution:</span>
              <span class="resolution-text">{decision.resolution}</span>
            </div>
          {/if}

          {#if decision.rationale}
            <div class="decision-rationale">
              <span class="rationale-label">Rationale:</span>
              <p class="rationale-text">{decision.rationale}</p>
            </div>
          {/if}

          {#if decision.options && decision.options.length > 0}
            <div class="decision-options">
              <span class="options-label">Options Considered:</span>
              <div class="options-list">
                {#each decision.options as option}
                  <div class="option-item">
                    <div class="option-label">{option.label}</div>
                    <div class="option-description">{option.description}</div>
                    {#if option.pros && option.pros.length > 0}
                      <div class="option-pros">
                        <span class="pros-label">Pros:</span>
                        <ul>
                          {#each option.pros as pro}
                            <li>{pro}</li>
                          {/each}
                        </ul>
                      </div>
                    {/if}
                    {#if option.cons && option.cons.length > 0}
                      <div class="option-cons">
                        <span class="cons-label">Cons:</span>
                        <ul>
                          {#each option.cons as con}
                            <li>{con}</li>
                          {/each}
                        </ul>
                      </div>
                    {/if}
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>

        <!-- Decision Footer -->
        <div class="decision-footer">
          <div class="decision-meta-info">
            {#if decision.decided_by}
              <span class="meta-item">
                <span class="meta-label">Decided by:</span>
                <span class="meta-value">{decision.decided_by}</span>
              </span>
            {/if}

            {#if decision.approved_by}
              <span class="meta-item">
                <span class="meta-label">Approved by:</span>
                <span class="meta-value">{decision.approved_by}</span>
              </span>
            {/if}

            <span class="meta-item">
              <span class="meta-label">Thread:</span>
              <span class="meta-value monospace">{decision.thread_id}</span>
            </span>

            {#if decision.decided_at}
              <span class="meta-item">
                <span class="meta-label">At:</span>
                <span class="meta-value">{formatDate(decision.decided_at)}</span>
              </span>
            {/if}
          </div>
        </div>
      </div>
    {/each}
  </div>
</div>

<style>
  .decision-log {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  /* Header */
  .log-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 1.5rem;
    background: white;
    border-radius: 8px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
  }

  .header-title h2 {
    margin: 0 0 0.5rem 0;
    font-size: 1.5rem;
    font-weight: 600;
    color: #111827;
  }

  .stats {
    display: flex;
    gap: 1rem;
    font-size: 0.875rem;
  }

  .stat-item {
    padding: 0.25rem 0.75rem;
    border-radius: 9999px;
    font-weight: 500;
  }

  .stat-item.pending {
    background: #fef3c7;
    color: #92400e;
  }

  .stat-item.approved {
    background: #d1fae5;
    color: #065f46;
  }

  .stat-item.rejected {
    background: #fee2e2;
    color: #991b1b;
  }

  .stat-item.superseded {
    background: #f3f4f6;
    color: #374151;
  }

  .refresh-btn {
    padding: 0.5rem 1rem;
    border: 1px solid #d1d5db;
    background: white;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.875rem;
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
  .log-filters {
    display: flex;
    gap: 1rem;
    padding: 1rem 1.5rem;
    background: white;
    border-radius: 8px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
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

  .filter-group input,
  .filter-group select {
    padding: 0.5rem;
    border: 1px solid #d1d5db;
    border-radius: 6px;
    font-size: 0.875rem;
  }

  .filter-group input {
    width: 300px;
  }

  .filter-group select {
    min-width: 150px;
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

  /* Decision List */
  .decision-list {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .decision-card {
    background: white;
    border: 1px solid #e5e7eb;
    border-radius: 8px;
    padding: 1.5rem;
    transition: all 0.2s;
  }

  .decision-card:hover {
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
  }

  .decision-header {
    display: flex;
    justify-content: space-between;
    align-items: start;
    margin-bottom: 1rem;
    padding-bottom: 1rem;
    border-bottom: 1px solid #f3f4f6;
  }

  .decision-meta {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .decision-icon {
    font-size: 1.25rem;
  }

  .decision-id {
    font-size: 0.75rem;
    color: #9ca3af;
    font-family: 'SF Mono', Monaco, monospace;
  }

  .decision-status {
    font-size: 0.875rem;
    font-weight: 600;
    text-transform: capitalize;
  }

  .decision-actions {
    display: flex;
    gap: 0.5rem;
  }

  .action-btn {
    padding: 0.375rem 0.75rem;
    border: 1px solid #d1d5db;
    background: white;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.875rem;
    transition: all 0.2s;
  }

  .action-btn:hover {
    border-color: #9ca3af;
  }

  .action-btn.approve:hover {
    background: #d1fae5;
    border-color: #10b981;
  }

  .action-btn.reject:hover {
    background: #fee2e2;
    border-color: #ef4444;
  }

  .action-btn.view:hover {
    background: #dbeafe;
    border-color: #3b82f6;
  }

  /* Decision Content */
  .decision-content {
    margin-bottom: 1rem;
  }

  .decision-question {
    margin: 0 0 1rem 0;
    font-size: 1.125rem;
    font-weight: 600;
    color: #111827;
  }

  .decision-resolution,
  .decision-rationale {
    margin-bottom: 1rem;
  }

  .resolution-label,
  .rationale-label {
    font-weight: 600;
    color: #374151;
    display: block;
    margin-bottom: 0.25rem;
  }

  .resolution-text {
    color: #4b5563;
  }

  .rationale-text {
    color: #4b5563;
    line-height: 1.6;
    margin: 0;
    white-space: pre-wrap;
  }

  .decision-options {
    margin-top: 1rem;
    padding: 1rem;
    background: #f9fafb;
    border-radius: 6px;
  }

  .options-label {
    font-weight: 600;
    color: #374151;
    display: block;
    margin-bottom: 0.75rem;
  }

  .options-list {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .option-item {
    padding: 1rem;
    background: white;
    border: 1px solid #e5e7eb;
    border-radius: 6px;
  }

  .option-label {
    font-weight: 600;
    color: #111827;
    margin-bottom: 0.25rem;
  }

  .option-description {
    color: #4b5563;
    margin-bottom: 0.5rem;
  }

  .option-pros,
  .option-cons {
    margin-top: 0.5rem;
  }

  .pros-label {
    color: #10b981;
    font-weight: 500;
    font-size: 0.875rem;
  }

  .cons-label {
    color: #ef4444;
    font-weight: 500;
    font-size: 0.875rem;
  }

  .option-pros ul,
  .option-cons ul {
    margin: 0.25rem 0 0 1.5rem;
    padding: 0;
  }

  .option-pros li,
  .option-cons li {
    font-size: 0.875rem;
    color: #4b5563;
    margin-bottom: 0.125rem;
  }

  /* Footer */
  .decision-footer {
    padding-top: 1rem;
    border-top: 1px solid #f3f4f6;
  }

  .decision-meta-info {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem;
    font-size: 0.875rem;
  }

  .meta-item {
    display: flex;
    align-items: center;
    gap: 0.25rem;
  }

  .meta-label {
    color: #6b7280;
    font-weight: 500;
  }

  .meta-value {
    color: #374151;
  }

  .monospace {
    font-family: 'SF Mono', Monaco, monospace;
  }
</style>
