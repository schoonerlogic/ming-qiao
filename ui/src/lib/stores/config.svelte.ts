/**
 * Configuration state management with Svelte 5 runes
 */

import { api } from '$lib/api';
import type { ConfigResponse, ObservationMode } from '$lib/types';

// ============================================================================
// State
// ============================================================================

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

// ============================================================================
// Actions
// ============================================================================

export async function loadConfig() {
  loading = true;
  error = null;

  try {
    config = await api.getConfig();
  } catch (e) {
    error = e instanceof Error ? e.message : 'Failed to load config';
    console.error('Error loading config:', e);
  } finally {
    loading = false;
  }
}

export async function setMode(mode: ObservationMode) {
  try {
    config = await api.updateConfig({ mode });
  } catch (e) {
    error = e instanceof Error ? e.message : 'Failed to update mode';
    console.error('Error setting mode:', e);
    throw e;
  }
}

export function updateLocalMode(mode: ObservationMode) {
  // Update local state without API call (for WebSocket updates)
  config.mode = mode;
}

// ============================================================================
// Derived State
// ============================================================================

export const configStore = {
  get config() {
    return config;
  },
  get mode() {
    return config.mode;
  },
  get notifyOn() {
    return config.notify_on;
  },
  get loading() {
    return loading;
  },
  get error() {
    return error;
  },
  get isPassive() {
    return config.mode === 'passive';
  },
  get isAdvisory() {
    return config.mode === 'advisory';
  },
  get isGated() {
    return config.mode === 'gated';
  },
};
