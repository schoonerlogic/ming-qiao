<script lang="ts">
  import { merlinNotifications } from '$stores/merlinNotifications';
  import type { MerlinNotificationUI } from '$lib/types/notifications';
  import { getNotificationConfig, getNotificationColorClass } from '$lib/types/notifications';

  /**
   * NotificationCenter - Bell icon with notification sidebar/drawer
   * Shows unread badge count and displays notification list
   */

  let isOpen = $state(false);

  // Get reactive state from store
  let unreadCount = $derived(merlinNotifications.unreadCount);
  let notifications = $derived(merlinNotifications.notifications);
  let connected = $derived(merlinNotifications.connected);

  // Format timestamp for display
  function formatTime(timestamp: string): string {
    try {
      const date = new Date(timestamp);
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
    } catch {
      return 'Unknown time';
    }
  }

  // Handle notification click
  function handleNotificationClick(notification: MerlinNotificationUI) {
    // Mark as read
    if (!notification.read) {
      merlinNotifications.markAsRead(notification.id);
    }

    // Handle navigation based on notification type
    if (notification.type === 'decisionReview' && notification.event.thread_id) {
      // Navigate to thread with decision
      // TODO: Implement navigation to thread
      console.log('[NotificationCenter] Navigate to thread:', notification.event.thread_id);
    } else if (notification.type === 'keywordDetected' && notification.event.thread_id) {
      // Navigate to thread with keyword match
      console.log('[NotificationCenter] Navigate to thread:', notification.event.thread_id);
    }
  }

  // Handle dismiss button click (stop propagation to avoid triggering notification click)
  function handleDismiss(event: Event, notification: MerlinNotificationUI) {
    event.stopPropagation();
    merlinNotifications.dismiss(notification.id);
  }

  // Mark all as read
  function markAllAsRead() {
    merlinNotifications.markAllAsRead();
  }

  // Dismiss all
  function dismissAll() {
    merlinNotifications.dismissAll();
  }

  // Close drawer when clicking outside
  function handleBackdropClick() {
    isOpen = false;
  }
</script>

<div class="notification-center">
  <!-- Bell Icon Button -->
  <button
    class="bell-button"
    onclick={() => (isOpen = !isOpen)}
    aria-label="Notifications"
    title={connected ? 'Notifications connected' : 'Notifications disconnected'}
  >
    <span class="bell-icon">🔔</span>
    
    {#if unreadCount > 0}
      <span class="badge" class:badge-pulse={unreadCount > 0}>
        {unreadCount}
      </span>
    {/if}
    
    <span class="connection-indicator" class:connected={connected} class:disconnected={!connected}></span>
  </button>

  <!-- Sidebar Drawer -->
  {#if isOpen}
    <div class="backdrop" onclick={handleBackdropClick}></div>
    <div class="drawer">
      <!-- Header -->
      <div class="drawer-header">
        <h2>Notifications</h2>
        <div class="header-actions">
          {#if unreadCount > 0}
            <button class="text-button" onclick={markAllAsRead}>Mark all read</button>
          {/if}
          {#if notifications.length > 0}
            <button class="text-button" onclick={dismissAll}>Clear all</button>
          {/if}
          <button class="close-button" onclick={() => (isOpen = false)} aria-label="Close notifications">
            ✕
          </button>
        </div>
      </div>

      <!-- Notification List -->
      <div class="notification-list">
        {#if notifications.length === 0}
          <div class="empty-state">
            <p class="empty-icon">🔔</p>
            <p>No notifications</p>
            {#if !connected}
              <p class="connection-status">Connecting...</p>
            {/if}
          </div>
        {:else}
          {#each notifications as notification (notification.id)}
            {@const config = getNotificationConfig(notification.type)}
            {@const colorClass = getNotificationColorClass(notification.type)}
            
            <div
              class="notification-item {colorClass}"
              class:unread={!notification.read}
              onclick={() => handleNotificationClick(notification)}
              role="button"
              tabindex="0"
              onkeydown={(e) => e.key === 'Enter' && handleNotificationClick(notification)}
            >
              <div class="notification-content">
                <div class="notification-header">
                  <span class="notification-icon">{config.icon}</span>
                  <span class="notification-time">{formatTime(notification.receivedAt)}</span>
                  <button
                    class="dismiss-button"
                    onclick={(e) => handleDismiss(e, notification)}
                    aria-label="Dismiss notification"
                  >
                    ✕
                  </button>
                </div>

                <div class="notification-body">
                  {#if notification.type === 'connected'}
                    <p class="notification-message">{notification.message}</p>
                    <p class="notification-meta">Mode: {notification.mode}</p>
                  {:else if notification.type === 'priorityAlert'}
                    <p class="notification-title">Priority Alert ({notification.priority})</p>
                    <p class="notification-message">{notification.reason}</p>
                    {#if notification.event.agent}
                      <p class="notification-meta">From: {notification.event.agent}</p>
                    {/if}
                  {:else if notification.type === 'keywordDetected'}
                    <p class="notification-title">Keyword Detected</p>
                    <p class="notification-message">
                      <strong>"{notification.keyword}"</strong>
                    </p>
                    <p class="notification-meta">By {notification.event.agent}</p>
                    <p class="notification-preview">{notification.event.message.slice(0, 100)}...</p>
                  {:else if notification.type === 'decisionReview'}
                    <p class="notification-title">Decision Review Required</p>
                    <p class="notification-message">{notification.event.question}</p>
                    <p class="notification-meta">
                      {notification.event.options.length} option{notification.event.options.length === 1 ? '' : 's'}
                    </p>
                  {:else if notification.type === 'actionBlocked'}
                    <p class="notification-title">Action Blocked</p>
                    <p class="notification-message">{notification.reason}</p>
                    <p class="notification-meta">By {notification.event.agent}</p>
                  {:else if notification.type === 'statusUpdate'}
                    <p class="notification-message">{notification.message}</p>
                  {/if}
                </div>
              </div>
            </div>
          {/each}
        {/if}
      </div>

      <!-- Footer -->
      {#if connected}
        <div class="drawer-footer">
          <span class="connection-status connected">● Connected</span>
        </div>
      {:else}
        <div class="drawer-footer">
          <span class="connection-status disconnected">● Reconnecting...</span>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .notification-center {
    position: relative;
    display: inline-block;
  }

  /* Bell Button */
  .bell-button {
    position: relative;
    background: none;
    border: none;
    cursor: pointer;
    padding: 0.5rem;
    border-radius: 50%;
    transition: background-color 0.2s;
  }

  .bell-button:hover {
    background-color: rgba(0, 0, 0, 0.05);
  }

  .bell-icon {
    font-size: 1.5rem;
    display: block;
  }

  /* Badge */
  .badge {
    position: absolute;
    top: 0;
    right: 0;
    background-color: #ef4444;
    color: white;
    border-radius: 9999px;
    padding: 0.125rem 0.375rem;
    font-size: 0.75rem;
    font-weight: bold;
    min-width: 1.25rem;
    text-align: center;
    animation: badge-in 0.3s ease-out;
  }

  .badge-pulse {
    animation: badge-in 0.3s ease-out, pulse 2s infinite;
  }

  @keyframes badge-in {
    from {
      transform: scale(0);
    }
    to {
      transform: scale(1);
    }
  }

  @keyframes pulse {
    0%, 100% {
      opacity: 1;
    }
    50% {
      opacity: 0.7;
    }
  }

  /* Connection Indicator */
  .connection-indicator {
    position: absolute;
    bottom: 0;
    right: 0;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    border: 2px solid white;
  }

  .connection-indicator.connected {
    background-color: #22c55e;
  }

  .connection-indicator.disconnected {
    background-color: #ef4444;
  }

  /* Backdrop */
  .backdrop {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background-color: rgba(0, 0, 0, 0.5);
    z-index: 999;
  }

  /* Drawer */
  .drawer {
    position: fixed;
    top: 0;
    right: 0;
    bottom: 0;
    width: 400px;
    max-width: 90vw;
    background-color: white;
    box-shadow: -4px 0 12px rgba(0, 0, 0, 0.15);
    z-index: 1000;
    display: flex;
    flex-direction: column;
    animation: slide-in 0.3s ease-out;
  }

  @keyframes slide-in {
    from {
      transform: translateX(100%);
    }
    to {
      transform: translateX(0);
    }
  }

  /* Drawer Header */
  .drawer-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 1rem;
    border-bottom: 1px solid #e5e7eb;
  }

  .drawer-header h2 {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 600;
  }

  .header-actions {
    display: flex;
    gap: 0.5rem;
    align-items: center;
  }

  .text-button {
    background: none;
    border: none;
    color: #3b82f6;
    cursor: pointer;
    font-size: 0.875rem;
    padding: 0.25rem 0.5rem;
    border-radius: 0.25rem;
    transition: background-color 0.2s;
  }

  .text-button:hover {
    background-color: #eff6ff;
  }

  .close-button {
    background: none;
    border: none;
    cursor: pointer;
    font-size: 1.25rem;
    padding: 0.25rem;
    border-radius: 0.25rem;
    transition: background-color 0.2s;
  }

  .close-button:hover {
    background-color: rgba(0, 0, 0, 0.05);
  }

  /* Notification List */
  .notification-list {
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem;
  }

  .empty-state {
    text-align: center;
    padding: 3rem 1rem;
    color: #6b7280;
  }

  .empty-icon {
    font-size: 3rem;
    margin-bottom: 0.5rem;
  }

  .connection-status {
    font-size: 0.875rem;
    margin-top: 0.5rem;
  }

  /* Notification Item */
  .notification-item {
    border: 1px solid;
    border-radius: 0.5rem;
    padding: 0.75rem;
    margin-bottom: 0.5rem;
    cursor: pointer;
    transition: all 0.2s;
  }

  .notification-item:hover {
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
  }

  .notification-item.unread {
    font-weight: 500;
  }

  .notification-content {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .notification-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .notification-icon {
    font-size: 1.25rem;
  }

  .notification-time {
    font-size: 0.75rem;
    color: #6b7280;
    flex: 1;
  }

  .dismiss-button {
    background: none;
    border: none;
    cursor: pointer;
    font-size: 1rem;
    opacity: 0.5;
    transition: opacity 0.2s;
    padding: 0.125rem;
  }

  .dismiss-button:hover {
    opacity: 1;
  }

  .notification-body {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .notification-title {
    font-weight: 600;
    margin: 0;
  }

  .notification-message {
    margin: 0;
    line-height: 1.4;
  }

  .notification-meta {
    font-size: 0.75rem;
    color: #6b7280;
    margin: 0;
  }

  .notification-preview {
    font-size: 0.875rem;
    color: #6b7280;
    margin: 0;
    font-style: italic;
  }

  /* Drawer Footer */
  .drawer-footer {
    padding: 0.75rem 1rem;
    border-top: 1px solid #e5e7eb;
    font-size: 0.875rem;
  }

  .connection-status.connected {
    color: #22c55e;
  }

  .connection-status.disconnected {
    color: #ef4444;
  }

  /* Responsive */
  @media (max-width: 640px) {
    .drawer {
      width: 100%;
      max-width: 100vw;
    }
  }
</style>
