/**
 * Merlin notification store using Svelte 5 runes
 * Manages WebSocket connection to /merlin/notifications
 */

import { browser } from '$app/environment';
import type { MerlinNotification, MerlinNotificationUI, MerlinIntervention, Toast } from '$lib/types/notifications';
import { getNotificationConfig } from '$lib/types/notifications';

// ============================================================================
// WebSocket URL
// ============================================================================

const MERLIN_NOTIFICATIONS_URL = 'ws://localhost:7777/merlin/notifications';

// ============================================================================
// Notification Store
// ============================================================================

// Skip SSR execution by wrapping in browser check
let socket: WebSocket | null = $state(browser ? null : null);
let connected = $state(browser ? false : false);
let connectionError = $state<string | null>(browser ? null : null);
let notifications = $state<MerlinNotificationUI[]>(browser ? [] : []);
let unreadCount = $state(browser ? 0 : 0);

// Reconnection tracking (prevent infinite loops)
let isReconnecting = $state(browser ? false : false);
let reconnectAttempts = $state(browser ? 0 : 0);
const MAX_RECONNECT_ATTEMPTS = 10;

// Auto-dismissal tracking
const autoDismissTimers = new Map<string, ReturnType<typeof setTimeout>>();

// ============================================================================
// Connection Management
// ============================================================================

/**
 * Connect to Merlin notification WebSocket
 */
export function connect() {
  if (!browser) return; // Skip SSR
  
  // Prevent re-entrant calls
  if (isReconnecting) {
    console.log('[MerlinNotifications] Already reconnecting, skipping');
    return;
  }
  
  if (socket?.readyState === WebSocket.OPEN) {
    console.log('[MerlinNotifications] Already connected');
    return;
  }

  // Check max retry limit
  if (reconnectAttempts >= MAX_RECONNECT_ATTEMPTS) {
    console.log('[MerlinNotifications] Max reconnection attempts reached, giving up');
    connectionError = 'Connection failed after multiple attempts';
    isReconnecting = false;
    return;
  }

  console.log(`[MerlinNotifications] Connecting to ${MERLIN_NOTIFICATIONS_URL} (attempt ${reconnectAttempts + 1}/${MAX_RECONNECT_ATTEMPTS})`);
  connectionError = null;
  isReconnecting = true;
  reconnectAttempts++;

  try {
    socket = new WebSocket(MERLIN_NOTIFICATIONS_URL);

    socket.onopen = () => {
      console.log('[MerlinNotifications] Connected');
      connected = true;
      connectionError = null;
      isReconnecting = false;
      reconnectAttempts = 0; // Reset on successful connection
    };

    socket.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        console.log('[MerlinNotifications] Received:', data);
        handleNotification(data);
      } catch (error) {
        console.error('[MerlinNotifications] Failed to parse message:', error);
      }
    };

    socket.onerror = (error) => {
      console.error('[MerlinNotifications] WebSocket error:', error);
      connectionError = 'Connection error';
    };

    socket.onclose = () => {
      console.log('[MerlinNotifications] Disconnected');
      connected = false;
      socket = null;
      isReconnecting = false; // Allow reconnection

      // Auto-reconnect after 5 seconds (only if under max attempts)
      if (reconnectAttempts < MAX_RECONNECT_ATTEMPTS) {
        setTimeout(() => {
          if (!connected) {
            console.log('[MerlinNotifications] Attempting to reconnect...');
            connect();
          }
        }, 5000);
      } else {
        console.log('[MerlinNotifications] Max reconnection attempts reached, stopping');
        connectionError = 'Connection failed. Please refresh the page.';
      }
    };
  } catch (error) {
    console.error('[MerlinNotifications] Failed to connect:', error);
    connectionError = 'Failed to connect';
    isReconnecting = false;
  }
}

/**
 * Disconnect from Merlin notification WebSocket
 */
export function disconnect() {
  console.log('[MerlinNotifications] Disconnecting');
  
  // Clear all auto-dismiss timers
  autoDismissTimers.forEach((timer) => clearTimeout(timer));
  autoDismissTimers.clear();

  if (socket) {
    socket.close();
    socket = null;
  }
  
  connected = false;
}

/**
 * Send a message to the Merlin notification stream
 */
export function send(data: unknown) {
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    console.error('[MerlinNotifications] Cannot send: not connected');
    return false;
  }

  try {
    socket.send(JSON.stringify(data));
    return true;
  } catch (error) {
    console.error('[MerlinNotifications] Failed to send:', error);
    return false;
  }
}

// ============================================================================
// Notification Management
// ============================================================================

/**
 * Handle incoming notification from WebSocket
 */
function handleNotification(data: unknown) {
  const notification = data as MerlinNotification;
  const config = getNotificationConfig(notification.type);

  // Create UI notification with state
  const uiNotification: MerlinNotificationUI = {
    ...notification,
    id: crypto.randomUUID(),
    read: false,
    dismissed: false,
    receivedAt: new Date().toISOString()
  };

  // Add to notifications list (at the beginning)
  notifications = [uiNotification, ...notifications];

  // Update unread count
  unreadCount++;

  // Set up auto-dismissal if not sticky
  if (!config.sticky && config.duration > 0) {
    const timer = setTimeout(() => {
      dismiss(uiNotification.id);
    }, config.duration);
    autoDismissTimers.set(uiNotification.id, timer);
  }
}

/**
 * Mark notification as read
 */
export function markAsRead(id: string) {
  const notification = notifications.find((n) => n.id === id);
  if (notification && !notification.read) {
    notification.read = true;
    unreadCount = Math.max(0, unreadCount - 1);
  }
}

/**
 * Mark all notifications as read
 */
export function markAllAsRead() {
  notifications.forEach((n) => {
    if (!n.read) {
      n.read = true;
    }
  });
  unreadCount = 0;
}

/**
 * Dismiss a notification
 */
export function dismiss(id: string) {
  const timer = autoDismissTimers.get(id);
  if (timer) {
    clearTimeout(timer);
    autoDismissTimers.delete(id);
  }

  const notification = notifications.find((n) => n.id === id);
  if (notification) {
    notification.dismissed = true;
    
    // Update unread count if not read
    if (!notification.read) {
      unreadCount = Math.max(0, unreadCount - 1);
    }
  }

  // Remove from array after transition
  setTimeout(() => {
    notifications = notifications.filter((n) => n.id !== id);
  }, 300);
}

/**
 * Dismiss all notifications
 */
export function dismissAll() {
  // Clear all timers
  autoDismissTimers.forEach((timer) => clearTimeout(timer));
  autoDismissTimers.clear();

  notifications.forEach((n) => {
    n.dismissed = true;
  });

  // Clear all after transition
  setTimeout(() => {
    notifications = [];
    unreadCount = 0;
  }, 300);
}

/**
 * Get non-dismissed notifications, sorted by priority
 */
export function getActiveNotifications(): MerlinNotificationUI[] {
  return notifications
    .filter((n) => !n.dismissed)
    .sort((a, b) => {
      const configA = getNotificationConfig(a.type);
      const configB = getNotificationConfig(b.type);
      return configB.priority - configA.priority;
    });
}

/**
 * Subscribe to notification updates (simple implementation)
 * Returns an unsubscribe function
 */
export function subscribeToNotifications(callback: (notification: MerlinNotificationUI) => void): () => void {
  // For v0.1, we'll use a simple approach
  // The callback will be called directly from handleNotification
  // This is a placeholder for future enhancement
  let active = true;
  
  // Store the subscriber
  const subscriber = {
    callback,
    get isActive() { return active; }
  };
  
  // For now, return an unsubscribe function
  return () => {
    active = false;
  };
}

// ============================================================================
// Intervention & Toast System
// ============================================================================

let toasts = $state<Toast[]>(browser ? [] : []);
const toastTimers = new Map<string, ReturnType<typeof setTimeout>>();

/**
 * Send a Merlin intervention via WebSocket
 */
export function sendIntervention(intervention: MerlinIntervention): boolean {
  if (!socket || socket.readyState !== WebSocket.OPEN) {
    showToast({
      type: 'error',
      message: 'Not connected to server',
      duration: 10000
    });
    return false;
  }

  try {
    socket.send(JSON.stringify(intervention));
    console.log('[MerlinNotifications] Sent intervention:', intervention);
    return true;
  } catch (error) {
    console.error('[MerlinNotifications] Failed to send intervention:', error);
    showToast({
      type: 'error',
      message: `Failed to send ${intervention.action}: ${error}`,
      duration: 10000
    });
    return false;
  }
}

/**
 * Show a toast notification
 */
export function showToast(toast: Omit<Toast, 'id'>): string {
  const id = crypto.randomUUID();
  const newToast: Toast = {
    ...toast,
    id
  };

  // Add to toasts list
  toasts = [...toasts, newToast];

  // Set up auto-dismissal
  if (toast.duration > 0) {
    const timer = setTimeout(() => {
      dismissToast(id);
    }, toast.duration);
    toastTimers.set(id, timer);
  }

  return id;
}

/**
 * Dismiss a toast notification
 */
export function dismissToast(id: string) {
  const timer = toastTimers.get(id);
  if (timer) {
    clearTimeout(timer);
    toastTimers.delete(id);
  }

  toasts = toasts.filter((t) => t.id !== id);
}

/**
 * Get all active toasts
 */
export function getActiveToasts(): Toast[] {
  return toasts;
}

// ============================================================================
// Store Export (reactive)
// ============================================================================

export const merlinNotifications = {
  // State
  get connected() { return connected; },
  get connectionError() { return connectionError; },
  get notifications() { return getActiveNotifications(); },
  get unreadCount() { return unreadCount; },
  get toasts() { return getActiveToasts(); },

  // Methods
  connect,
  disconnect,
  send,
  sendIntervention,
  markAsRead,
  markAllAsRead,
  dismiss,
  dismissAll,
  subscribeToNotifications,
  showToast,
  dismissToast
};

// ============================================================================
// Auto-connect on import (optional, remove if you want manual connection)
// ============================================================================

// Note: Auto-connect removed because $effect cannot be used at module level
// The component using this store should call connect() in its onMount
// Example: in MerlinNotificationStream.svelte or +page.svelte

