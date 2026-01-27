/**
 * WebSocket connection management with Svelte 5 runes
 */

import type { WSMessage, WSConnected, ObservationMode } from '$lib/types';

// ============================================================================
// State
// ============================================================================

let ws = $state<WebSocket | null>(null);
let connected = $state(false);
let reconnectAttempts = $state(0);
let maxReconnectAttempts = 5;
let reconnectDelay = 1000; // Start with 1 second

const WS_URL = 'ws://localhost:3000/ws';

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
    const message = JSON.parse(event.data) as WSMessage;

    // Notify all registered handlers
    for (const handler of handlers) {
      handler(message);
    }
  } catch (e) {
    console.error('Error parsing WebSocket message:', e);
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
// WebSocket Message Helpers
// ============================================================================

export function injectMessage(threadId: string, content: string, action: string = 'comment') {
  send({
    type: 'inject',
    thread_id: threadId,
    content,
    action,
  });
}

export function approveDecision(decisionId: string) {
  send({
    type: 'approve',
    decision_id: decisionId,
  });
}

export function rejectDecision(decisionId: string, reason: string) {
  send({
    type: 'reject',
    decision_id: decisionId,
    reason,
  });
}

export function setMode(mode: ObservationMode) {
  send({
    type: 'set_mode',
    mode,
  });
}

export function subscribeToThread(threadId: string) {
  send({
    type: 'subscribe',
    thread_id: threadId,
  });
}

export function markMessageRead(messageId: string) {
  send({
    type: 'mark_read',
    message_id: messageId,
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
