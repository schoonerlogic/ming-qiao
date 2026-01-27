/**
 * Thread state management with Svelte 5 runes
 */

import { api } from '$lib/api';
import type { Thread, ThreadDetail } from '$lib/types';

// ============================================================================
// State
// ============================================================================

let threads = $state<Thread[]>([]);
let loading = $state(false);
let error = $state<string | null>(null);
let currentThread = $state<ThreadDetail | null>(null);

// ============================================================================
// Actions
// ============================================================================

export async function loadThreads(status: 'active' | 'paused' | 'resolved' | 'archived' | 'all' = 'active') {
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
}

export async function loadThread(id: string) {
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
}

export async function refreshThreads() {
  // Reload with current status filter (default to active for now)
  await loadThreads('active');
}

export function clearCurrentThread() {
  currentThread = null;
}

export function updateThreadInList(thread: Thread) {
  const index = threads.findIndex((t) => t.thread_id === thread.thread_id);
  if (index !== -1) {
    threads[index] = thread;
  } else {
    threads.unshift(thread);
  }
}

export function removeThreadFromList(threadId: string) {
  threads = threads.filter((t) => t.thread_id !== threadId);
}

// ============================================================================
// Derived State
// ============================================================================

export const threadsStore = {
  get threads() {
    return threads;
  },
  get loading() {
    return loading;
  },
  get error() {
    return error;
  },
  get currentThread() {
    return currentThread;
  },
  get unreadCount() {
    return threads.reduce((sum, t) => sum + t.unread_count, 0);
  },
  get activeThreads() {
    return threads.filter((t) => t.status === 'active');
  },
  get pausedThreads() {
    return threads.filter((t) => t.status === 'paused');
  },
};
