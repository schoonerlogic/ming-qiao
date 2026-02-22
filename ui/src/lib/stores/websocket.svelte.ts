/**
 * WebSocket connection management with Svelte 5 runes
 */

import type { WSMessage, WSConnected, ObservationMode, EventEnvelope } from '$lib/types';

// ============================================================================
// State
// ============================================================================

let ws = $state<WebSocket | null>(null);
let connected = $state(false);
let reconnectAttempts = $state(0);
let maxReconnectAttempts = 5;
let reconnectDelay = 1000; // Start with 1 second

const WS_URL = 'ws://localhost:7777/merlin/notifications';

// ============================================================================
// Event Handlers
// ============================================================================

type MessageHandler = (message: WSMessage) => void;

const handlers: MessageHandler[] = [];

export function onMessage(handler: MessageHandler) {
  handlers.push(handler);

  // Return cleanup function
  return () => {
    const index = handlers.indexOf(handler);
    if (index !== -1) {
      handlers.splice(index, 1);
    }
  };
}

function handleMessage(event: MessageEvent) {
  try {
    const raw = JSON.parse(event.data);
    console.log('WebSocket message received:', raw);

    // Handle the connected message specially
    if (raw.type === 'connected') {
      const message: WSMessage = raw as WSMessage;
      for (const handler of handlers) {
        handler(message);
      }
      return;
    }

    // Handle all Merlin notification types
    const message = raw as WSMessage;
    for (const handler of handlers) {
      handler(message);
    }
  } catch (e) {
    console.error('Error parsing WebSocket message:', event.data, e);
  }
}

// ============================================================================
// Connection Management
// ============================================================================

export function connect() {
  if (ws && (ws.readyState === WebSocket.CONNECTING || ws.readyState === WebSocket.OPEN)) {
    console.log('WebSocket already connected or connecting');
    return;
  }

  console.log(`Connecting to WebSocket at ${WS_URL}`);
  
  try {
    ws = new WebSocket(WS_URL);

    ws.onopen = () => {
      console.log('WebSocket connected');
      connected = true;
      reconnectAttempts = 0;
      reconnectDelay = 1000;
    };

    ws.onmessage = handleMessage;

    ws.onclose = (event) => {
      console.log(`WebSocket closed: ${event.code} ${event.reason}`);
      connected = false;
      ws = null;

      // Attempt to reconnect if not intentionally closed
      if (event.code !== 1000 && reconnectAttempts < maxReconnectAttempts) {
        const delay = reconnectDelay * Math.pow(2, reconnectAttempts);
        console.log(`Reconnecting in ${delay}ms (attempt ${reconnectAttempts + 1}/${maxReconnectAttempts})`);
        
        setTimeout(() => {
          reconnectAttempts++;
          connect();
        }, delay);
      } else if (reconnectAttempts >= maxReconnectAttempts) {
        console.error('Max reconnect attempts reached. Giving up.');
      }
    };

    ws.onerror = (error) => {
      console.error('WebSocket error:', error);
    };
  } catch (e) {
    console.error('Error creating WebSocket:', e);
  }
}

export function disconnect() {
  if (ws) {
    console.log('Disconnecting WebSocket');
    ws.close(1000, 'User disconnected');
    ws = null;
    connected = false;
  }
}

export function send(message: object) {
  if (!ws || ws.readyState !== WebSocket.OPEN) {
    console.error('WebSocket not connected. Cannot send message:', message);
    return;
  }

  try {
    ws.send(JSON.stringify(message));
  } catch (e) {
    console.error('Error sending WebSocket message:', e);
  }
}

// ============================================================================
// WebSocket Message Helpers (Merlin Interventions)
// ============================================================================

export function injectMessage(threadId: string, from: string, content: string) {
  send({
    action: 'inject_message',
    thread_id: threadId,
    from,
    content,
  });
}

export function approveDecision(decisionId: string, reason?: string) {
  send({
    action: 'approve_decision',
    decision_id: decisionId,
    reason,
  });
}

export function rejectDecision(decisionId: string, reason?: string) {
  send({
    action: 'reject_decision',
    decision_id: decisionId,
    reason,
  });
}

export function setMode(mode: ObservationMode) {
  send({
    action: 'set_mode',
    mode,
  });
}

// ============================================================================
// Derived State
// ============================================================================

export const wsStore = {
  get connected() {
    return connected;
  },
  get reconnectAttempts() {
    return reconnectAttempts;
  },
  get ws() {
    return ws;
  },
};
