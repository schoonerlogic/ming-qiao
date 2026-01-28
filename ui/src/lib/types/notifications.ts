/**
 * TypeScript types for Merlin notification system
 * Based on Rust types in src/merlin/notifier.rs and src/events/schema.rs
 */

import type { Priority, ObservationMode } from '../types';

// ============================================================================
// Merlin Notification Types
// ============================================================================

/**
 * All possible Merlin notification types from the WebSocket
 */
export type MerlinNotification =
  | ConnectedNotification
  | PriorityAlertNotification
  | KeywordDetectedNotification
  | DecisionReviewNotification
  | ActionBlockedNotification
  | StatusUpdateNotification;

/**
 * Initial connection confirmation with current observation mode
 */
export interface ConnectedNotification {
  type: 'connected';
  message: string;
  mode: ObservationMode;
  timestamp: string;
}

/**
 * Priority-based alert when important events occur
 * Triggered by high/critical priority messages or decisions
 */
export interface PriorityAlertNotification {
  type: 'priorityAlert';
  event_id: string;
  priority: Priority;
  reason: string;
  event: {
    id: string;
    timestamp: string;
    event_type: string;
    agent?: string;
    summary: string;
  };
  timestamp: string;
}

/**
 * Triggered when configured keywords are detected in messages
 */
export interface KeywordDetectedNotification {
  type: 'keywordDetected';
  event_id: string;
  keyword: string;
  event: {
    id: string;
    timestamp: string;
    agent: string;
    message: string;
    thread_id?: string;
  };
  timestamp: string;
}

/**
 * Request for Merlin to review a decision before execution
 */
export interface DecisionReviewNotification {
  type: 'decisionReview';
  event_id: string;
  decision_type: string;
  event: {
    id: string;
    timestamp: string;
    decision_id: string;
    question: string;
    options: Array<{
      label: string;
      description?: string;
      pros?: string[];
      cons?: string[];
    }>;
    thread_id?: string;
  };
  timestamp: string;
}

/**
 * Action was blocked due to gated observation mode
 */
export interface ActionBlockedNotification {
  type: 'actionBlocked';
  event_id: string;
  reason: string;
  event: {
    id: string;
    timestamp: string;
    event_type: string;
    agent: string;
    description: string;
  };
  timestamp: string;
}

/**
 * General status updates from the system
 */
export interface StatusUpdateNotification {
  type: 'statusUpdate';
  message: string;
  timestamp: string;
}

// ============================================================================
// Notification UI State
// ============================================================================

/**
 * Extended notification with UI state (read/dismissed tracking)
 * Uses intersection type instead of extending the union
 */
export type MerlinNotificationUI = (
  | ConnectedNotification
  | PriorityAlertNotification
  | KeywordDetectedNotification
  | DecisionReviewNotification
  | ActionBlockedNotification
  | StatusUpdateNotification
) & {
  id: string; // Unique UI ID (not the same as event_id)
  read: boolean;
  dismissed: boolean;
  receivedAt: string; // When UI received the notification
};

/**
 * Notification display configuration by type
 */
export interface NotificationConfig {
  color: string;
  icon: string;
  sticky: boolean;
  duration: number; // milliseconds
  priority: number; // For sorting (higher = more important)
}

/**
 * Mapping of notification types to their display configuration
 */
export const NOTIFICATION_CONFIGS: Record<MerlinNotification['type'], NotificationConfig> = {
  connected: {
    color: 'gray',
    icon: '📋',
    sticky: false,
    duration: 10000, // 10 seconds
    priority: 0
  },
  priorityAlert: {
    color: 'red',
    icon: '🔔',
    sticky: true,
    duration: 0, // Never auto-hide
    priority: 100
  },
  keywordDetected: {
    color: 'orange',
    icon: '🔍',
    sticky: false,
    duration: 30000, // 30 seconds
    priority: 75
  },
  decisionReview: {
    color: 'purple',
    icon: '⚖️',
    sticky: true,
    duration: 0, // Never auto-hide
    priority: 90
  },
  actionBlocked: {
    color: 'red',
    icon: '🚫',
    sticky: true,
    duration: 0, // Never auto-hide
    priority: 95
  },
  statusUpdate: {
    color: 'gray',
    icon: '📋',
    sticky: false,
    duration: 10000, // 10 seconds
    priority: 10
  }
};

/**
 * Helper to get notification config
 */
export function getNotificationConfig(type: MerlinNotification['type']): NotificationConfig {
  return NOTIFICATION_CONFIGS[type];
}

/**
 * Helper to determine if notification should be sticky
 */
export function isStickyNotification(notification: MerlinNotification): boolean {
  return getNotificationConfig(notification.type).sticky;
}

/**
 * Helper to get notification color class (Tailwind)
 */
export function getNotificationColorClass(type: MerlinNotification['type']): string {
  const color = getNotificationConfig(type).color;
  const colorMap: Record<string, string> = {
    red: 'bg-red-50 border-red-200 text-red-800',
    orange: 'bg-orange-50 border-orange-200 text-orange-800',
    purple: 'bg-purple-50 border-purple-200 text-purple-800',
    blue: 'bg-blue-50 border-blue-200 text-blue-800',
    gray: 'bg-gray-50 border-gray-200 text-gray-800'
  };
  return colorMap[color] || colorMap.gray;
}

// ============================================================================
// Merlin Intervention Types
// ============================================================================

/**
 * Intervention actions that Merlin can send via WebSocket
 */
export type MerlinIntervention =
  | InjectMessageIntervention
  | ApproveDecisionIntervention
  | RejectDecisionIntervention
  | SetModeIntervention;

/**
 * Inject a message into a thread
 */
export interface InjectMessageIntervention {
  action: 'inject_message';  // Backend expects snake_case
  threadId: string;
  from: string; // Agent ID (usually "merlin")
  content: string;
}

/**
 * Approve a pending decision
 */
export interface ApproveDecisionIntervention {
  action: 'approve_decision';  // Backend expects snake_case
  decisionId: string;
  reason?: string;
}

/**
 * Reject a pending decision
 */
export interface RejectDecisionIntervention {
  action: 'reject_decision';  // Backend expects snake_case
  decisionId: string;
  reason?: string;
}

/**
 * Change observation mode
 */
export interface SetModeIntervention {
  action: 'set_mode';  // Backend expects snake_case
  mode: 'passive' | 'advisory' | 'gated';
}

// ============================================================================
// Toast Notification Types
// ============================================================================

/**
 * Toast notification types for user feedback
 */
export type ToastType = 'success' | 'error' | 'warning' | 'info';

/**
 * Toast notification configuration
 */
export interface Toast {
  id: string;
  type: ToastType;
  message: string;
  duration: number; // milliseconds, 0 = sticky
}
