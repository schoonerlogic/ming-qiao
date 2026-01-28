/**
 * Message state management with Svelte 5 runes
 * 
 * NOTE: This file uses .svelte.ts extension to enable $state runes
 * The state is wrapped in functions to avoid SSR execution
 */

import { api } from '$lib/api';
import type { Message, InboxResponse } from '$lib/types';

// ============================================================================
// Store Implementation
// ============================================================================

function createMessageStore() {
  let messages = $state<Message[]>([]);
  let inbox = $state<InboxResponse | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);

  return {
    get messages() { return messages; },
    get inbox() { return inbox; },
    get loading() { return loading; },
    get error() { return error; },
    
    async loadInbox(agent: string, unreadOnly: boolean = true, limit: number = 20, from?: string) {
      loading = true;
      error = null;

      try {
        const response = await api.getInbox(agent, unreadOnly, limit, from);
        inbox = response;
        messages = response.messages;
      } catch (e) {
        error = e instanceof Error ? e.message : 'Failed to load inbox';
        console.error('Error loading inbox:', e);
      } finally {
        loading = false;
      }
    },

    addMessage(message: Message) {
      messages.push(message);
    }
  };
}

// ============================================================================
// Singleton Instance (lazy, browser-only)
// ============================================================================

let store: ReturnType<typeof createMessageStore> | null = null;

function getStore() {
  if (!store) {
    store = createMessageStore();
  }
  return store;
}

// ============================================================================
// Public API
// ============================================================================

export const messageStore = {
  get messages() { return getStore().messages; },
  get inbox() { return getStore().inbox; },
  get loading() { return getStore().loading; },
  get error() { return getStore().error; }
};

export async function loadInbox(agent: string, unreadOnly?: boolean, limit?: number, from?: string) {
  return getStore().loadInbox(agent, unreadOnly, limit, from);
}

export function addMessage(message: Message) {
  return getStore().addMessage(message);
}
