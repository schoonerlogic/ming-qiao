/**
 * Message state management with Svelte 5 runes
 */

import { api } from '$lib/api';
import type { Message, InboxResponse } from '$lib/types';

// ============================================================================
// State
// ============================================================================

let messages = $state<Message[]>([]);
let inbox = $state<InboxResponse | null>(null);
let loading = $state(false);
let error = $state<string | null>(null);

// ============================================================================
// Actions
// ============================================================================

export async function loadInbox(
  agent: string,
  unreadOnly: boolean = true,
  limit: number = 20,
  from?: string
) {
  loading = true;
  error = null;

  try {
    inbox = await api.getInbox(agent, unreadOnly, limit, from);
    messages = inbox.messages;
  } catch (e) {
    error = e instanceof Error ? e.message : 'Failed to load inbox';
    console.error('Error loading inbox:', e);
  } finally {
    loading = false;
  }
}

export async function markAsRead(messageId: string) {
  try {
    await api.markMessageRead(messageId);

    // Update local state
    const message = messages.find((m) => m.message_id === messageId);
    if (message) {
      message.read_at = new Date().toISOString();
    }

    // Update inbox counts
    if (inbox && inbox.unread_count > 0) {
      inbox.unread_count--;
    }
  } catch (e) {
    console.error('Error marking message as read:', e);
    throw e;
  }
}

export function addMessage(message: Message) {
  // Add or update message in local state
  const index = messages.findIndex((m) => m.message_id === message.message_id);
  if (index !== -1) {
    messages[index] = message;
  } else {
    messages.unshift(message);
  }
}

export function updateMessage(messageId: string, updates: Partial<Message>) {
  const message = messages.find((m) => m.message_id === messageId);
  if (message) {
    Object.assign(message, updates);
  }
}

// ============================================================================
// Derived State
// ============================================================================

export const messagesStore = {
  get messages() {
    return messages;
  },
  get inbox() {
    return inbox;
  },
  get loading() {
    return loading;
  },
  get error() {
    return error;
  },
  get unreadCount() {
    return inbox?.unread_count || 0;
  },
  get totalCount() {
    return inbox?.total_count || 0;
  },
  get highPriorityMessages() {
    return messages.filter((m) => m.priority === 'high' || m.priority === 'critical');
  },
};
