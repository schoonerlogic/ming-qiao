/**
 * Thread state management with Svelte 5 runes
 * 
 * NOTE: This file uses .svelte.ts extension to enable $state runes
 * The state is wrapped in functions to avoid SSR execution
 */

import { api } from '$lib/api';
import type { Thread, ThreadDetail } from '$lib/types';

// ============================================================================
// Store Implementation
// ============================================================================

function createThreadStore() {
  let threads = $state<Thread[]>([]);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let currentThread = $state<ThreadDetail | null>(null);

  return {
    get threads() { return threads; },
    get loading() { return loading; },
    get error() { return error; },
    get currentThread() { return currentThread; },
    
    async loadThreads(status: 'active' | 'paused' | 'resolved' | 'archived' | 'all' = 'active') {
      loading = true;
      error = null;

      try {
        const response = await api.getThreads(status);
        threads = response.threads;
      } catch (e) {
        error = e instanceof Error ? e.message : 'Failed to load threads';
        console.error('Error loading threads:', e);
      } finally {
        loading = false;
      }
    },

    async loadThread(id: string) {
      loading = true;
      error = null;

      try {
        currentThread = await api.getThread(id);
      } catch (e) {
        error = e instanceof Error ? e.message : 'Failed to load thread';
        console.error('Error loading thread:', e);
      } finally {
        loading = false;
      }
    },

    refreshThreads() {
      return this.loadThreads('active');
    },

    clearCurrentThread() {
      currentThread = null;
    },

    updateThreadInList(thread: Thread) {
      const index = threads.findIndex((t) => t.thread_id === thread.thread_id);
      if (index !== -1) {
        threads[index] = thread;
      } else {
        threads.push(thread);
      }
    }
  };
}

// ============================================================================
// Singleton Instance (lazy, browser-only)
// ============================================================================

let store: ReturnType<typeof createThreadStore> | null = null;

function getStore() {
  if (!store) {
    store = createThreadStore();
  }
  return store;
}

// ============================================================================
// Public API
// ============================================================================

export const threadsStore = {
  get threads() { return getStore().threads; },
  get loading() { return getStore().loading; },
  get error() { return getStore().error; },
  get currentThread() { return getStore().currentThread; }
};

export async function loadThreads(status: 'active' | 'paused' | 'resolved' | 'archived' | 'all' = 'active') {
  return getStore().loadThreads(status);
}

export async function loadThread(id: string) {
  return getStore().loadThread(id);
}

export async function refreshThreads() {
  return getStore().refreshThreads();
}

export function clearCurrentThread() {
  return getStore().clearCurrentThread();
}

export function updateThreadInList(thread: Thread) {
  return getStore().updateThreadInList(thread);
}
