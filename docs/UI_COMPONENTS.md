# Ming-Qiao UI Components Specification

**Framework:** Svelte 5  
**Build Tool:** Vite  
**Styling:** Tailwind CSS  
**Location:** `ui/`

---

## Overview

The Merlin Dashboard is a single-page application for observing and managing agent conversations. It provides real-time updates via WebSocket and allows Merlin to intervene in threads.

---

## Layout

```
┌─────────────────────────────────────────────────────────────────┐
│  Header                                            [Mode ▾]     │
├────────────────┬────────────────────────────────────────────────┤
│                │                                                │
│  ThreadList    │  ThreadView / DecisionView / SearchResults     │
│                │                                                │
│  (sidebar)     │  (main content)                                │
│                │                                                │
│                │                                                │
│                │                                                │
│                │                                                │
│                │                                                │
├────────────────┴────────────────────────────────────────────────┤
│  MerlinInput                                                    │
└─────────────────────────────────────────────────────────────────┘
```

---

## Components

### App.svelte

Root component. Manages layout and routing.

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import Header from '$lib/Header.svelte';
  import ThreadList from '$lib/ThreadList.svelte';
  import ThreadView from '$lib/ThreadView.svelte';
  import MerlinInput from '$lib/MerlinInput.svelte';
  import { threads, selectedThread } from './stores/threads';
  import { connect } from './stores/websocket';

  onMount(() => {
    connect();
  });
</script>

<div class="h-screen flex flex-col bg-gray-900 text-gray-100">
  <Header />
  
  <div class="flex-1 flex overflow-hidden">
    <aside class="w-80 border-r border-gray-700 overflow-y-auto">
      <ThreadList />
    </aside>
    
    <main class="flex-1 flex flex-col overflow-hidden">
      {#if $selectedThread}
        <ThreadView thread={$selectedThread} />
      {:else}
        <div class="flex-1 flex items-center justify-center text-gray-500">
          Select a thread to view
        </div>
      {/if}
    </main>
  </div>
  
  <MerlinInput />
</div>
```

---

### Header.svelte

Top bar with title, mode toggle, and notifications.

```svelte
<script lang="ts">
  import ModeToggle from '$lib/ModeToggle.svelte';
  import SearchBar from '$lib/SearchBar.svelte';
  import { unreadCount } from './stores/threads';
</script>

<header class="h-14 px-4 flex items-center justify-between border-b border-gray-700 bg-gray-800">
  <div class="flex items-center gap-4">
    <h1 class="text-lg font-semibold">明桥 Ming-Qiao</h1>
    <SearchBar />
  </div>
  
  <div class="flex items-center gap-4">
    {#if $unreadCount > 0}
      <span class="px-2 py-1 bg-blue-600 rounded-full text-sm">
        {$unreadCount} unread
      </span>
    {/if}
    <ModeToggle />
  </div>
</header>
```

**Props:** None

**State:**
- Reads `unreadCount` from store

---

### ModeToggle.svelte

Dropdown to switch observation modes.

```svelte
<script lang="ts">
  import { config, setMode } from './stores/config';
  
  const modes = ['passive', 'advisory', 'gated'];
  let open = false;
  
  function select(mode: string) {
    setMode(mode);
    open = false;
  }
</script>

<div class="relative">
  <button 
    class="px-3 py-1 rounded border border-gray-600 hover:bg-gray-700"
    on:click={() => open = !open}
  >
    {$config.mode}
    <span class="ml-2">▾</span>
  </button>
  
  {#if open}
    <div class="absolute right-0 mt-1 w-32 bg-gray-800 border border-gray-600 rounded shadow-lg">
      {#each modes as mode}
        <button
          class="w-full px-3 py-2 text-left hover:bg-gray-700"
          class:bg-gray-700={mode === $config.mode}
          on:click={() => select(mode)}
        >
          {mode}
        </button>
      {/each}
    </div>
  {/if}
</div>
```

---

### SearchBar.svelte

Global search input.

```svelte
<script lang="ts">
  import { goto } from '$app/navigation';
  
  let query = '';
  
  function search() {
    if (query.trim()) {
      goto(`/search?q=${encodeURIComponent(query)}`);
    }
  }
</script>

<form on:submit|preventDefault={search} class="flex">
  <input
    type="text"
    bind:value={query}
    placeholder="Search messages & decisions..."
    class="w-64 px-3 py-1 bg-gray-700 border border-gray-600 rounded-l focus:outline-none focus:border-blue-500"
  />
  <button 
    type="submit"
    class="px-3 py-1 bg-gray-600 rounded-r hover:bg-gray-500"
  >
    🔍
  </button>
</form>
```

---

### ThreadList.svelte

Sidebar showing all threads.

```svelte
<script lang="ts">
  import { threads, selectedThreadId, selectThread } from './stores/threads';
  import ThreadItem from '$lib/ThreadItem.svelte';
  
  let filter: 'active' | 'resolved' | 'all' = 'active';
  
  $: filteredThreads = $threads.filter(t => {
    if (filter === 'all') return true;
    if (filter === 'active') return ['active', 'paused', 'blocked'].includes(t.status);
    return t.status === 'resolved';
  });
</script>

<div class="flex flex-col h-full">
  <div class="p-3 border-b border-gray-700">
    <select 
      bind:value={filter}
      class="w-full px-2 py-1 bg-gray-700 border border-gray-600 rounded"
    >
      <option value="active">Active</option>
      <option value="resolved">Resolved</option>
      <option value="all">All</option>
    </select>
  </div>
  
  <div class="flex-1 overflow-y-auto">
    {#each filteredThreads as thread (thread.thread_id)}
      <ThreadItem 
        {thread} 
        selected={thread.thread_id === $selectedThreadId}
        on:click={() => selectThread(thread.thread_id)}
      />
    {/each}
    
    {#if filteredThreads.length === 0}
      <p class="p-4 text-gray-500">No threads</p>
    {/if}
  </div>
</div>
```

---

### ThreadItem.svelte

Single thread in the sidebar list.

```svelte
<script lang="ts">
  import type { Thread } from '../types';
  
  export let thread: Thread;
  export let selected: boolean = false;
  
  const agentColors: Record<string, string> = {
    aleph: 'bg-green-500',
    thales: 'bg-blue-500',
    merlin: 'bg-purple-500'
  };
</script>

<button
  class="w-full p-3 text-left border-b border-gray-700 hover:bg-gray-800"
  class:bg-gray-800={selected}
  on:click
>
  <div class="flex items-start justify-between">
    <span class="font-medium truncate">{thread.subject}</span>
    {#if thread.unread_count > 0}
      <span class="ml-2 px-1.5 py-0.5 bg-blue-600 rounded-full text-xs">
        {thread.unread_count}
      </span>
    {/if}
  </div>
  
  <div class="mt-1 flex items-center gap-2 text-sm text-gray-400">
    <div class="flex -space-x-1">
      {#each thread.participants as p}
        <span 
          class="w-4 h-4 rounded-full {agentColors[p] || 'bg-gray-500'}"
          title={p}
        />
      {/each}
    </div>
    <span>·</span>
    <span>{formatTime(thread.last_message_at)}</span>
  </div>
  
  {#if thread.status !== 'active'}
    <span class="mt-1 inline-block px-1.5 py-0.5 text-xs rounded
      {thread.status === 'paused' ? 'bg-yellow-600' : ''}
      {thread.status === 'blocked' ? 'bg-red-600' : ''}
      {thread.status === 'resolved' ? 'bg-green-600' : ''}
    ">
      {thread.status}
    </span>
  {/if}
</button>

<script context="module" lang="ts">
  function formatTime(iso: string): string {
    const d = new Date(iso);
    const now = new Date();
    if (d.toDateString() === now.toDateString()) {
      return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    }
    return d.toLocaleDateString([], { month: 'short', day: 'numeric' });
  }
</script>
```

---

### ThreadView.svelte

Main content area showing thread messages and decisions.

```svelte
<script lang="ts">
  import type { Thread, Message, Decision } from '../types';
  import MessageComponent from '$lib/Message.svelte';
  import DecisionCard from '$lib/DecisionCard.svelte';
  
  export let thread: Thread;
  
  // Interleave messages and decisions by timestamp
  $: items = [...thread.messages, ...thread.decisions]
    .sort((a, b) => {
      const aTime = 'sent_at' in a ? a.sent_at : a.decided_at;
      const bTime = 'sent_at' in b ? b.sent_at : b.decided_at;
      return new Date(aTime).getTime() - new Date(bTime).getTime();
    });
</script>

<div class="flex-1 flex flex-col overflow-hidden">
  <!-- Thread header -->
  <div class="p-4 border-b border-gray-700 bg-gray-800">
    <h2 class="text-lg font-semibold">{thread.subject}</h2>
    <div class="mt-1 text-sm text-gray-400">
      {thread.participants.join(', ')} · {thread.message_count} messages
      {#if thread.decision_count > 0}
        · {thread.decision_count} decisions
      {/if}
    </div>
  </div>
  
  <!-- Messages -->
  <div class="flex-1 overflow-y-auto p-4 space-y-4">
    {#each items as item (item.message_id || item.decision_id)}
      {#if 'message_id' in item}
        <MessageComponent message={item} />
      {:else}
        <DecisionCard decision={item} />
      {/if}
    {/each}
  </div>
</div>
```

---

### Message.svelte

Single message display.

```svelte
<script lang="ts">
  import type { Message } from '../types';
  import { marked } from 'marked';
  
  export let message: Message;
  
  const agentColors: Record<string, string> = {
    aleph: 'border-green-500',
    thales: 'border-blue-500',
    merlin: 'border-purple-500'
  };
  
  $: html = marked.parse(message.content);
</script>

<div class="flex gap-3">
  <div class="w-8 h-8 rounded-full flex items-center justify-center text-sm font-bold
    {message.from_agent === 'aleph' ? 'bg-green-600' : ''}
    {message.from_agent === 'thales' ? 'bg-blue-600' : ''}
    {message.from_agent === 'merlin' ? 'bg-purple-600' : ''}
  ">
    {message.from_agent[0].toUpperCase()}
  </div>
  
  <div class="flex-1">
    <div class="flex items-baseline gap-2">
      <span class="font-semibold">{message.from_agent}</span>
      <span class="text-sm text-gray-400">{formatTime(message.sent_at)}</span>
      {#if message.priority === 'high' || message.priority === 'critical'}
        <span class="px-1.5 py-0.5 text-xs rounded bg-red-600">
          {message.priority}
        </span>
      {/if}
    </div>
    
    <div class="mt-1 prose prose-invert prose-sm max-w-none">
      {@html html}
    </div>
    
    {#if message.artifact_refs?.length > 0}
      <div class="mt-2 flex flex-wrap gap-2">
        {#each message.artifact_refs as ref}
          <a 
            href="/api/artifacts/{ref.path}"
            target="_blank"
            class="px-2 py-1 text-sm bg-gray-700 rounded hover:bg-gray-600"
          >
            📎 {ref.path.split('/').pop()}
          </a>
        {/each}
      </div>
    {/if}
  </div>
</div>

<script context="module" lang="ts">
  function formatTime(iso: string): string {
    return new Date(iso).toLocaleTimeString([], { 
      hour: '2-digit', 
      minute: '2-digit' 
    });
  }
</script>
```

---

### DecisionCard.svelte

Decision display with approve/reject actions.

```svelte
<script lang="ts">
  import type { Decision } from '../types';
  import { approveDecision, rejectDecision } from './stores/decisions';
  
  export let decision: Decision;
  
  let rejectReason = '';
  let showRejectInput = false;
  
  async function approve() {
    await approveDecision(decision.decision_id);
  }
  
  async function reject() {
    if (!showRejectInput) {
      showRejectInput = true;
      return;
    }
    if (rejectReason.trim()) {
      await rejectDecision(decision.decision_id, rejectReason);
      showRejectInput = false;
    }
  }
</script>

<div class="p-4 rounded-lg border-2 
  {decision.status === 'pending' ? 'border-yellow-500 bg-yellow-900/20' : ''}
  {decision.status === 'approved' ? 'border-green-500 bg-green-900/20' : ''}
  {decision.status === 'rejected' ? 'border-red-500 bg-red-900/20' : ''}
">
  <div class="flex items-start justify-between">
    <div>
      <span class="text-xs uppercase tracking-wide text-gray-400">Decision</span>
      <h3 class="font-semibold">{decision.question}</h3>
    </div>
    <span class="px-2 py-1 text-xs rounded
      {decision.status === 'pending' ? 'bg-yellow-600' : ''}
      {decision.status === 'approved' ? 'bg-green-600' : ''}
      {decision.status === 'rejected' ? 'bg-red-600' : ''}
    ">
      {decision.status}
    </span>
  </div>
  
  <div class="mt-3">
    <p class="text-sm text-gray-400">Resolution:</p>
    <p class="font-medium">{decision.resolution}</p>
  </div>
  
  <div class="mt-2">
    <p class="text-sm text-gray-400">Rationale:</p>
    <p class="text-sm">{decision.rationale}</p>
  </div>
  
  <div class="mt-2 text-sm text-gray-400">
    Decided by {decision.decided_by} · {formatDate(decision.decided_at)}
  </div>
  
  {#if decision.status === 'pending'}
    <div class="mt-4 flex gap-2">
      <button 
        class="px-3 py-1 bg-green-600 rounded hover:bg-green-500"
        on:click={approve}
      >
        Approve
      </button>
      <button 
        class="px-3 py-1 bg-red-600 rounded hover:bg-red-500"
        on:click={reject}
      >
        Reject
      </button>
    </div>
    
    {#if showRejectInput}
      <div class="mt-2">
        <input 
          type="text"
          bind:value={rejectReason}
          placeholder="Reason for rejection..."
          class="w-full px-3 py-1 bg-gray-700 border border-gray-600 rounded"
        />
      </div>
    {/if}
  {/if}
</div>

<script context="module" lang="ts">
  function formatDate(iso: string): string {
    return new Date(iso).toLocaleString([], {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
    });
  }
</script>
```

---

### MerlinInput.svelte

Bottom input bar for Merlin to inject messages.

```svelte
<script lang="ts">
  import { selectedThreadId } from './stores/threads';
  import { inject } from './stores/websocket';
  
  let content = '';
  let action: 'comment' | 'pause' | 'redirect' = 'comment';
  
  function send() {
    if (!content.trim() || !$selectedThreadId) return;
    
    inject($selectedThreadId, content, action);
    content = '';
  }
  
  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      send();
    }
  }
  
  // Slash commands
  $: if (content.startsWith('/pause ')) {
    action = 'pause';
    content = content.slice(7);
  } else if (content.startsWith('/redirect ')) {
    action = 'redirect';
  } else {
    action = 'comment';
  }
</script>

<div class="p-3 border-t border-gray-700 bg-gray-800">
  <div class="flex gap-2">
    <select 
      bind:value={action}
      class="px-2 py-1 bg-gray-700 border border-gray-600 rounded text-sm"
    >
      <option value="comment">Comment</option>
      <option value="pause">Pause</option>
      <option value="redirect">Redirect</option>
    </select>
    
    <input
      type="text"
      bind:value={content}
      on:keydown={handleKeydown}
      placeholder={$selectedThreadId 
        ? "Inject message... (Cmd+Enter to send)" 
        : "Select a thread first"}
      disabled={!$selectedThreadId}
      class="flex-1 px-3 py-1 bg-gray-700 border border-gray-600 rounded 
             focus:outline-none focus:border-blue-500 disabled:opacity-50"
    />
    
    <button 
      on:click={send}
      disabled={!content.trim() || !$selectedThreadId}
      class="px-4 py-1 bg-purple-600 rounded hover:bg-purple-500 
             disabled:opacity-50 disabled:cursor-not-allowed"
    >
      Send as Merlin
    </button>
  </div>
</div>
```

---

## Stores

### stores/threads.ts

```typescript
import { writable, derived } from 'svelte/store';
import type { Thread } from '../types';

export const threads = writable<Thread[]>([]);
export const selectedThreadId = writable<string | null>(null);

export const selectedThread = derived(
  [threads, selectedThreadId],
  ([$threads, $id]) => $threads.find(t => t.thread_id === $id) || null
);

export const unreadCount = derived(
  threads,
  $threads => $threads.reduce((sum, t) => sum + (t.unread_count || 0), 0)
);

export function selectThread(id: string) {
  selectedThreadId.set(id);
}

export function updateThread(thread: Thread) {
  threads.update(list => {
    const idx = list.findIndex(t => t.thread_id === thread.thread_id);
    if (idx >= 0) {
      list[idx] = thread;
    } else {
      list.unshift(thread);
    }
    return [...list];
  });
}
```

### stores/websocket.ts

```typescript
import { threads, updateThread } from './threads';
import { config } from './config';

let ws: WebSocket | null = null;

export function connect() {
  ws = new WebSocket(`ws://${location.host}/ws`);
  
  ws.onmessage = (event) => {
    const msg = JSON.parse(event.data);
    
    switch (msg.type) {
      case 'message':
        // Reload thread with new message
        loadThread(msg.thread_id);
        break;
      case 'decision_pending':
        loadThread(msg.decision.thread_id);
        break;
      case 'thread_status':
        updateThreadStatus(msg.thread_id, msg.status);
        break;
      case 'mode_changed':
        config.update(c => ({ ...c, mode: msg.new_mode }));
        break;
    }
  };
  
  ws.onclose = () => {
    setTimeout(connect, 3000); // Reconnect
  };
}

export function inject(threadId: string, content: string, action: string) {
  ws?.send(JSON.stringify({
    type: 'inject',
    thread_id: threadId,
    content,
    action
  }));
}

export function approve(decisionId: string) {
  ws?.send(JSON.stringify({
    type: 'approve',
    decision_id: decisionId
  }));
}

export function reject(decisionId: string, reason: string) {
  ws?.send(JSON.stringify({
    type: 'reject',
    decision_id: decisionId,
    reason
  }));
}

async function loadThread(id: string) {
  const res = await fetch(`/api/thread/${id}`);
  const thread = await res.json();
  updateThread(thread);
}
```

### stores/config.ts

```typescript
import { writable } from 'svelte/store';

interface Config {
  mode: 'passive' | 'advisory' | 'gated';
}

export const config = writable<Config>({ mode: 'advisory' });

export async function setMode(mode: string) {
  await fetch('/api/config', {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ mode })
  });
  config.update(c => ({ ...c, mode: mode as Config['mode'] }));
}
```

---

## Types

### types/index.ts

```typescript
export interface Thread {
  thread_id: string;
  subject: string;
  participants: string[];
  status: 'active' | 'paused' | 'blocked' | 'resolved' | 'archived';
  started_at: string;
  last_message_at: string;
  message_count: number;
  decision_count: number;
  unread_count: number;
  messages: Message[];
  decisions: Decision[];
}

export interface Message {
  message_id: string;
  thread_id: string;
  from_agent: string;
  to_agent: string;
  subject?: string;
  content: string;
  priority: 'low' | 'normal' | 'high' | 'critical';
  sent_at: string;
  read_at: string | null;
  artifact_refs: ArtifactRef[];
}

export interface Decision {
  decision_id: string;
  thread_id: string;
  question: string;
  resolution: string;
  rationale: string;
  options_considered?: string[];
  decided_by: string;
  approved_by: string | null;
  status: 'pending' | 'approved' | 'rejected' | 'superseded';
  decided_at: string;
}

export interface ArtifactRef {
  artifact_id: string;
  path: string;
  sha256: string;
}
```

---

## Build

```bash
cd ui
pnpm install
pnpm build  # outputs to dist/
```

Development:

```bash
pnpm dev  # runs on :5173, proxies API to :7777
```
