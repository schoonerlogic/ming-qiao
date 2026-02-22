/**
 * Thread state management with Svelte 5 runes
 */

import { api } from '$lib/api';
import type { Thread, ThreadDetail } from '$lib/types';

// ============================================================================
// State - Export as a reactive object
// ============================================================================

const state = $state({
  threads: [] as Thread[],
  loading: false,
  error: null as string | null,
  currentThread: null as ThreadDetail | null
});

// ============================================================================
// Actions
// ============================================================================

export async function loadThreads(status: 'active' | 'paused' | 'resolved' | 'archived' | 'all' = 'active') {
  state.loading = true;
  state.error = null;

  try {
    console.log('[threads store] Loading threads with status:', status);
    const response = await api.getThreads(status);
    console.log('[threads store] Loaded threads:', response.threads.length, 'threads');
    state.threads = response.threads;
  } catch (e) {
    state.error = e instanceof Error ? e.message : 'Failed to load threads';
    console.error('[threads store] Error loading threads:', e);
  } finally {
    state.loading = false;
  }
}

export async function loadThread(id: string) {
  state.loading = true;
  state.error = null;

  try {
    state.currentThread = await api.getThread(id);
  } catch (e) {
    state.error = e instanceof Error ? e.message : 'Failed to load thread';
    console.error('Error loading thread:', e);
  } finally {
    state.loading = false;
  }
}

export async function refreshThreads() {
  // Reload with current status filter (default to active for now)
  await loadThreads('active');
}

export function clearCurrentThread() {
  state.currentThread = null;
}

export function updateThreadInList(thread: Thread) {
  const index = state.threads.findIndex((t) => t.id === thread.id);
  if (index !== -1) {
    state.threads[index] = thread;
  } else {
    state.threads.unshift(thread);
  }
}

export function removeThreadFromList(threadId: string) {
  state.threads = state.threads.filter((t) => t.id !== threadId);
}

// ============================================================================
// Store Export - Return the state object for reactivity
// ============================================================================

export const threadsStore = state;

// Export state getters for convenience
export const threads = () => state.threads;
export const loading = () => state.loading;
export const error = () => state.error;
export const currentThread = () => state.currentThread;

