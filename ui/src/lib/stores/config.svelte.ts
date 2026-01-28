/**
 * Configuration state management with Svelte 5 runes
 * 
 * NOTE: This file uses .svelte.ts extension to enable $state runes
 * The state is wrapped in functions to avoid SSR execution
 */

import { api } from '$lib/api';
import type { ConfigResponse, ObservationMode } from '$lib/types';

// ============================================================================
// Store Implementation
// ============================================================================

function createConfigStore() {
  let config = $state<ConfigResponse>({
    mode: 'passive',
    notify_on: {
      priority: ['high', 'critical'],
      keywords: ['breaking change', 'security'],
      decision_type: ['architectural'],
    },
  });

  let loading = $state(false);
  let error = $state<string | null>(null);

  return {
    get config() { return config; },
    get loading() { return loading; },
    get error() { return error; },
    
    async loadConfig() {
      loading = true;
      error = null;

      try {
        const response = await api.getConfig();
        config = response;
      } catch (e) {
        error = e instanceof Error ? e.message : 'Failed to load config';
        console.error('Error loading config:', e);
      } finally {
        loading = false;
      }
    },

    async setMode(mode: ObservationMode) {
      loading = true;
      error = null;

      try {
        await api.setMode(mode);
        config.mode = mode;
      } catch (e) {
        error = e instanceof Error ? e.message : 'Failed to set mode';
        console.error('Error setting mode:', e);
      } finally {
        loading = false;
      }
    }
  };
}

// ============================================================================
// Singleton Instance (lazy, browser-only)
// ============================================================================

let store: ReturnType<typeof createConfigStore> | null = null;

function getStore() {
  if (!store) {
    store = createConfigStore();
  }
  return store;
}

// ============================================================================
// Public API
// ============================================================================

export const configStore = {
  get config() { return getStore().config; },
  get loading() { return getStore().loading; },
  get error() { return getStore().error; }
};

export async function loadConfig() {
  return getStore().loadConfig();
}

export async function setMode(mode: ObservationMode) {
  return getStore().setMode(mode);
}

export function updateLocalMode(mode: ObservationMode) {
  getStore().config.mode = mode;
}
