/**
 * WebSocket connection management with Svelte 5 runes
 * 
 * NOTE: This file uses .svelte.ts extension to enable $state runes
 * The state is wrapped in functions to avoid SSR execution
 */

import type { WSMessage, WSConnected, ObservationMode } from '$lib/types';

const WS_URL = 'ws://localhost:7777/ws';

// ============================================================================
// Store Implementation
// ============================================================================

type MessageHandler = (message: WSMessage) => void;

function createWebSocketStore() {
  let ws = $state<WebSocket | null>(null);
  let connected = $state(false);
  let reconnectAttempts = $state(0);
  let maxReconnectAttempts = 5;
  let reconnectDelay = 1000;
  const handlers: MessageHandler[] = [];

  function handleMessage(event: MessageEvent) {
    try {
      const message = JSON.parse(event.data) as WSMessage;
      for (const handler of handlers) {
        handler(message);
      }
    } catch (e) {
      console.error('Error parsing WebSocket message:', e);
    }
  }

  function scheduleReconnect() {
    if (reconnectAttempts >= maxReconnectAttempts) {
      console.log('Max reconnect attempts reached');
      return;
    }

    reconnectAttempts++;
    const delay = reconnectDelay * Math.pow(2, reconnectAttempts - 1);

    console.log(`Reconnecting in ${delay}ms... (attempt ${reconnectAttempts}/${maxReconnectAttempts})`);

    setTimeout(() => {
      connect();
    }, delay);
  }

  function connect() {
    if (ws && (ws.readyState === WebSocket.CONNECTING || ws.readyState === WebSocket.OPEN)) {
      console.log('WebSocket already connected or connecting');
      return;
    }

    console.log(`Connecting to WebSocket at ${WS_URL}...`);

    ws = new WebSocket(WS_URL);

    ws.onopen = () => {
      console.log('WebSocket connected');
      connected = true;
      reconnectAttempts = 0;
      reconnectDelay = 1000;
    };

    ws.onclose = (event) => {
      console.log('WebSocket closed:', event);
      connected = false;
      ws = null;
      scheduleReconnect();
    };

    ws.onerror = (error) => {
      console.error('WebSocket error:', error);
    };

    ws.onmessage = handleMessage;
  }

  function disconnect() {
    if (ws) {
      ws.close();
      ws = null;
      connected = false;
    }
  }

  function send(message: any) {
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(message));
    } else {
      console.error('Cannot send message: WebSocket not connected');
    }
  }

  return {
    get connected() { return connected; },
    get reconnectAttempts() { return reconnectAttempts; },
    
    connect,
    disconnect,
    send,
    
    onMessage(handler: MessageHandler) {
      handlers.push(handler);
      return () => {
        const index = handlers.indexOf(handler);
        if (index !== -1) {
          handlers.splice(index, 1);
        }
      };
    }
  };
}

// ============================================================================
// Singleton Instance (lazy, browser-only)
// ============================================================================

let store: ReturnType<typeof createWebSocketStore> | null = null;

function getStore() {
  if (!store) {
    store = createWebSocketStore();
  }
  return store;
}

// ============================================================================
// Public API
// ============================================================================

export const websocketStore = {
  get connected() { return getStore().connected; },
  get reconnectAttempts() { return getStore().reconnectAttempts; }
};

export function connect() {
  return getStore().connect();
}

export function disconnect() {
  return getStore().disconnect();
}

export function send(message: any) {
  return getStore().send(message);
}

export function onMessage(handler: MessageHandler) {
  return getStore().onMessage(handler);
}
