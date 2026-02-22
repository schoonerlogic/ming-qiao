<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import type { EventEnvelope } from '$lib/types';

  // Agent interface
  interface AgentStatus {
    agentId: string;
    displayName: string;
    status: 'available' | 'working' | 'blocked' | 'offline';
    lastSeen: string;
    currentTask?: string;
    unreadCount: number;
    recentActivity: number; // Number of recent events
  }

  // Known agents configuration
  const knownAgents: Record<string, { displayName: string; description: string }> = {
    merlin: { displayName: 'Merlin', description: 'Human operator' },
    proteus: { displayName: 'Proteus', description: 'Coordinator' },
    thales: { displayName: 'Thales', description: 'Architect' },
    aleph: { displayName: 'Aleph', description: 'Builder' },
    luban: { displayName: 'Luban', description: 'Craftsman' },
    'laozi-jung': { displayName: 'Laozi-Jung', description: 'Witness' },
  };

  // State
  let agents = $state<AgentStatus[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let lastUpdate = $state<string>('');

  // Activity tracking
  let activityLog = $state<Map<string, Date[]>>(new Map());
  const ACTIVITY_WINDOW = 5 * 60 * 1000; // 5 minutes

  onMount(() => {
    initializeAgents();
    loadAgentStatus();

    // Update every 30 seconds
    const interval = setInterval(loadAgentStatus, 30000);

    return () => clearInterval(interval);
  });

  function initializeAgents() {
    // Initialize all known agents
    agents = Object.entries(knownAgents).map(([agentId, info]) => ({
      agentId,
      displayName: info.displayName,
      status: 'offline' as const,
      lastSeen: '',
      unreadCount: 0,
      recentActivity: 0,
    }));

    // Initialize activity tracking
    Object.keys(knownAgents).forEach(agentId => {
      activityLog.set(agentId, []);
    });
  }

  async function loadAgentStatus() {
    loading = true;
    error = null;

    try {
      // Load unread counts from inbox for each agent
      await Promise.all(
        agents.map(async (agent) => {
          try {
            const inbox = await api.getInbox(agent.agentId, true, 1);
            agent.unreadCount = inbox.unread_count;

            // Determine status based on activity and unread
            const now = new Date();
            const activities = activityLog.get(agent.agentId) || [];
            const recentActivities = activities.filter(
              (activity) => now.getTime() - activity.getTime() < ACTIVITY_WINDOW
            );

            agent.recentActivity = recentActivities.length;

            // Update status based on activity
            if (recentActivities.length > 5) {
              agent.status = 'working';
            } else if (recentActivities.length > 0) {
              agent.status = 'available';
            } else {
              // Check last seen time if available
              const lastActivity = activities[activities.length - 1];
              if (lastActivity && now.getTime() - lastActivity.getTime() < 15 * 60 * 1000) {
                agent.status = 'available';
              } else {
                agent.status = 'offline';
              }
            }
          } catch (e) {
            // Agent might not have an inbox yet, that's okay
            console.warn(`Failed to load inbox for ${agent.agentId}:`, e);
          }
        })
      );

      lastUpdate = new Date().toLocaleTimeString();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load agent status';
      console.error('Error loading agent status:', e);
    } finally {
      loading = false;
    }
  }

  function updateAgentActivity(agentId: string, event: EventEnvelope) {
    const activities = activityLog.get(agentId) || [];
    activities.push(new Date(event.timestamp));

    // Keep only last 100 activities
    if (activities.length > 100) {
      activities.shift();
    }

    activityLog.set(agentId, activities);

    // Update agent in list
    const agent = agents.find(a => a.agentId === agentId);
    if (agent) {
      agent.lastSeen = event.timestamp;
      const now = new Date();
      const recentActivities = activities.filter(
        (activity) => now.getTime() - activity.getTime() < ACTIVITY_WINDOW
      );
      agent.recentActivity = recentActivities.length;

      if (recentActivities.length > 5) {
        agent.status = 'working';
      } else if (recentActivities.length > 0) {
        agent.status = 'available';
      }
    }
  }

  // Expose function for external updates (from Live Stream)
  export function updateFromEvent(event: EventEnvelope) {
    updateAgentActivity(event.agent_id, event);

    // Also update participants in the event
    if (event.payload?.to_agent) {
      updateAgentActivity(event.payload.to_agent, event);
    }
  }

  function getStatusColor(status: AgentStatus['status']): string {
    switch (status) {
      case 'available':
        return '#10b981'; // Green
      case 'working':
        return '#3b82f6'; // Blue
      case 'blocked':
        return '#f59e0b'; // Amber
      case 'offline':
        return '#6b7280'; // Gray
    }
  }

  function getStatusIcon(status: AgentStatus['status']): string {
    switch (status) {
      case 'available':
        return '🟢';
      case 'working':
        return '🔵';
      case 'blocked':
        return '⚠️';
      case 'offline':
        return '⚫';
    }
  }

  function formatLastSeen(agent: AgentStatus): string {
    if (!agent.lastSeen) return 'Unknown';

    const date = new Date(agent.lastSeen);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);

    if (diffMins < 1) return 'Just now';
    if (diffMins < 60) return `${diffMins}m ago`;
    if (diffMins < 1440) return `${Math.floor(diffMins / 60)}h ago`;
    return `${Math.floor(diffMins / 1440)}d ago`;
  }

  function getActivityLevel(agent: AgentStatus): string {
    if (agent.recentActivity > 10) return 'High';
    if (agent.recentActivity > 5) return 'Medium';
    if (agent.recentActivity > 0) return 'Low';
    return 'None';
  }

  function getActivityColor(activity: number): string {
    if (activity > 10) return '#ef4444';
    if (activity > 5) return '#f59e0b';
    if (activity > 0) return '#10b981';
    return '#e5e7eb';
  }
</script>

<div class="agent-status-panel">
  <!-- Header -->
  <div class="panel-header">
    <div class="header-left">
      <h2>Agent Status</h2>
      {#if lastUpdate}
        <span class="last-update">Updated {lastUpdate}</span>
      {/if}
    </div>

    <button
      class="refresh-btn"
      onclick={loadAgentStatus}
      disabled={loading}
      title="Refresh agent status"
    >
      {loading ? '⏳' : '🔄'}
    </button>
  </div>

  <!-- Error State -->
  {#if error}
    <div class="error-state">
      <div class="error-icon">⚠️</div>
      <p class="error-message">{error}</p>
    </div>
  {/if}

  <!-- Statistics Summary -->
  <div class="stats-summary">
    <div class="stat-card">
      <span class="stat-value">{agents.filter(a => a.status === 'available').length}</span>
      <span class="stat-label">Available</span>
    </div>
    <div class="stat-card">
      <span class="stat-value">{agents.filter(a => a.status === 'working').length}</span>
      <span class="stat-label">Working</span>
    </div>
    <div class="stat-card">
      <span class="stat-value">{agents.filter(a => a.unreadCount > 0).length}</span>
      <span class="stat-label">With Messages</span>
    </div>
    <div class="stat-card">
      <span class="stat-value">{agents.filter(a => a.status === 'offline').length}</span>
      <span class="stat-label">Offline</span>
    </div>
  </div>

  <!-- Agent List -->
  <div class="agent-list">
    {#each agents as agent (agent.agentId)}
      <div class="agent-card">
        <!-- Agent Header -->
        <div class="agent-header">
          <div class="agent-info">
            <div class="agent-avatar" style="background-color: {getStatusColor(agent.status)}20; border: 2px solid {getStatusColor(agent.status)}">
              <span class="agent-initials" style="color: {getStatusColor(agent.status)}">
                {agent.displayName.substring(0, 2).toUpperCase()}
              </span>
            </div>

            <div class="agent-details">
              <h3 class="agent-name">{agent.displayName}</h3>
              <p class="agent-id">@{agent.agentId}</p>
            </div>
          </div>

          <div class="agent-status">
            <span class="status-indicator" style="background-color: {getStatusColor(agent.status)}">
              {getStatusIcon(agent.status)}
            </span>
            <span class="status-text" style="color: {getStatusColor(agent.status)}">
              {agent.status}
            </span>
          </div>
        </div>

        <!-- Agent Metrics -->
        <div class="agent-metrics">
          <div class="metric">
            <span class="metric-label">Last Seen:</span>
            <span class="metric-value">{formatLastSeen(agent)}</span>
          </div>

          <div class="metric">
            <span class="metric-label">Activity:</span>
            <div class="activity-bar">
              <div
                class="activity-level"
                style="width: {Math.min(agent.recentActivity * 10, 100)}%; background-color: {getActivityColor(agent.recentActivity)}"
              ></div>
              <span class="activity-text">{getActivityLevel(agent)}</span>
            </div>
          </div>

          <div class="metric">
            <span class="metric-label">Unread:</span>
            <span class="metric-value {agent.unreadCount > 0 ? 'has-unread' : ''}">
              {agent.unreadCount} messages
            </span>
          </div>
        </div>

        <!-- Agent Actions -->
        <div class="agent-actions">
          <button
            class="action-btn"
            title="Send message"
            disabled={agent.status === 'offline'}
          >
            💬 Message
          </button>

          {#if agent.unreadCount > 0}
            <button
              class="action-btn primary"
              title="View inbox"
            >
              📥 Inbox ({agent.unreadCount})
            </button>
          {/if}
        </div>
      </div>
    {/each}
  </div>

  <!-- Panel Footer -->
  <div class="panel-footer">
    <p class="footer-text">
      ℹ️ Agent status is derived from recent activity. Real-time status tracking requires backend support.
    </p>
  </div>
</div>

<style>
  .agent-status-panel {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  /* Header */
  .panel-header {
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

  .last-update {
    font-size: 0.875rem;
    color: #6b7280;
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

  /* Error State */
  .error-state {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 1rem 1.5rem;
    background: #fee2e2;
    border: 1px solid #ef4444;
    border-radius: 8px;
    color: #991b1b;
  }

  .error-icon {
    font-size: 1.5rem;
  }

  .error-message {
    flex: 1;
    font-size: 0.875rem;
  }

  /* Stats Summary */
  .stats-summary {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
    gap: 1rem;
  }

  .stat-card {
    padding: 1rem;
    background: white;
    border-radius: 8px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
    text-align: center;
  }

  .stat-value {
    display: block;
    font-size: 2rem;
    font-weight: 700;
    color: #111827;
  }

  .stat-label {
    font-size: 0.875rem;
    color: #6b7280;
  }

  /* Agent List */
  .agent-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(350px, 1fr));
    gap: 1rem;
  }

  .agent-card {
    background: white;
    border: 1px solid #e5e7eb;
    border-radius: 8px;
    padding: 1.25rem;
    transition: all 0.2s;
  }

  .agent-card:hover {
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.1);
    border-color: #d1d5db;
  }

  /* Agent Header */
  .agent-header {
    display: flex;
    justify-content: space-between;
    align-items: start;
    margin-bottom: 1rem;
    padding-bottom: 1rem;
    border-bottom: 1px solid #f3f4f6;
  }

  .agent-info {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .agent-avatar {
    width: 48px;
    height: 48px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .agent-initials {
    font-weight: 700;
    font-size: 1rem;
  }

  .agent-details {
    display: flex;
    flex-direction: column;
  }

  .agent-name {
    margin: 0;
    font-size: 1.125rem;
    font-weight: 600;
    color: #111827;
  }

  .agent-id {
    margin: 0;
    font-size: 0.75rem;
    color: #9ca3af;
    font-family: 'SF Mono', Monaco, monospace;
  }

  .agent-status {
    display: flex;
    flex-direction: column;
    align-items: end;
    gap: 0.25rem;
  }

  .status-indicator {
    font-size: 1.25rem;
    width: 32px;
    height: 32px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .status-text {
    font-size: 0.75rem;
    font-weight: 600;
    text-transform: capitalize;
  }

  /* Agent Metrics */
  .agent-metrics {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .metric {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 0.875rem;
  }

  .metric-label {
    color: #6b7280;
    font-weight: 500;
  }

  .metric-value {
    color: #374151;
    font-weight: 500;
  }

  .metric-value.has-unread {
    color: #ef4444;
    font-weight: 600;
  }

  .activity-bar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex: 1;
    max-width: 150px;
  }

  .activity-level {
    height: 6px;
    border-radius: 3px;
    transition: width 0.3s ease;
  }

  .activity-text {
    font-size: 0.75rem;
    color: #6b7280;
    font-weight: 500;
  }

  /* Agent Actions */
  .agent-actions {
    display: flex;
    gap: 0.5rem;
  }

  .action-btn {
    flex: 1;
    padding: 0.5rem;
    border: 1px solid #d1d5db;
    background: white;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.875rem;
    font-weight: 500;
    transition: all 0.2s;
  }

  .action-btn:hover:not(:disabled) {
    background: #f3f4f6;
    border-color: #9ca3af;
  }

  .action-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .action-btn.primary {
    background: #3b82f6;
    color: white;
    border-color: #3b82f6;
  }

  .action-btn.primary:hover:not(:disabled) {
    background: #2563eb;
    border-color: #2563eb;
  }

  /* Footer */
  .panel-footer {
    padding: 1rem 1.5rem;
    background: #f9fafb;
    border-radius: 8px;
  }

  .footer-text {
    margin: 0;
    font-size: 0.875rem;
    color: #6b7280;
    text-align: center;
  }
</style>
