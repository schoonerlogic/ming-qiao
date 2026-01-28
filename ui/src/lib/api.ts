/**
 * HTTP API client for Ming-Qiao backend
 */

import type {
  InboxResponse,
  ThreadsResponse,
  ThreadDetail,
  Thread,
  Message,
  Decision,
  DecisionsResponse,
  ArtifactsResponse,
  Artifact,
  ConfigResponse,
  SearchResponse,
  ReplyRequest,
  CreateThreadRequest,
  InjectRequest,
  AnnotateRequest,
  UpdateConfigRequest,
  Priority,
  ThreadStatus,
} from './types';

const API_BASE = 'http://localhost:7777';

export class ApiClient {
  private baseUrl: string;

  constructor(baseUrl: string = API_BASE) {
    this.baseUrl = baseUrl;
  }

  private async request<T>(
    path: string,
    options: RequestInit = {}
  ): Promise<T> {
    const url = `${this.baseUrl}${path}`;
    
    console.log(`[API] ${options.method || 'GET'} ${url}`);
    
    const response = await fetch(url, {
      headers: {
        'Content-Type': 'application/json',
        ...options.headers,
      },
      ...options,
    });

    console.log(`[API] Response status: ${response.status}`);

    if (!response.ok) {
      const error = await response.json().catch(() => ({
        error: {
          code: 'UNKNOWN',
          message: `HTTP ${response.status}`,
        },
      }));
      console.error('[API] Error response:', error);
      throw new Error(error.error?.message || 'Request failed');
    }

    const data = await response.json();
    console.log('[API] Response data:', data);
    return data;
  }

  // ============================================================================
  // Inbox
  // ============================================================================

  async getInbox(
    agent: string,
    unreadOnly: boolean = true,
    limit: number = 20,
    from?: string
  ): Promise<InboxResponse> {
    const params = new URLSearchParams({
      unread_only: String(unreadOnly),
      limit: String(limit),
    });

    if (from) {
      params.append('from', from);
    }

    return this.request<InboxResponse>(`/api/inbox/${agent}?${params}`);
  }

  // ============================================================================
  // Threads
  // ============================================================================

  async getThreads(
    status: ThreadStatus | 'all' = 'active',
    participant?: string,
    limit: number = 20,
    offset: number = 0
  ): Promise<ThreadsResponse> {
    const params = new URLSearchParams({
      status,
      limit: String(limit),
      offset: String(offset),
    });

    if (participant) {
      params.append('participant', participant);
    }

    return this.request<ThreadsResponse>(`/api/threads?${params}`);
  }

  async getThread(id: string): Promise<ThreadDetail> {
    return this.request<ThreadDetail>(`/api/thread/${id}`);
  }

  async replyToThread(id: string, reply: ReplyRequest): Promise<Message> {
    return this.request<Message>(`/api/thread/${id}/reply`, {
      method: 'POST',
      body: JSON.stringify(reply),
    });
  }

  async createThread(thread: CreateThreadRequest): Promise<Thread> {
    return this.request<Thread>('/api/threads', {
      method: 'POST',
      body: JSON.stringify(thread),
    });
  }

  async updateThreadStatus(
    id: string,
    status: ThreadStatus,
    reason?: string
  ): Promise<void> {
    return this.request<void>(`/api/thread/${id}`, {
      method: 'PATCH',
      body: JSON.stringify({ status, reason }),
    });
  }

  // ============================================================================
  // Messages
  // ============================================================================

  async getMessage(id: string): Promise<Message> {
    return this.request<Message>(`/api/message/${id}`);
  }

  async markMessageRead(id: string): Promise<void> {
    return this.request<void>(`/api/message/${id}`, {
      method: 'PATCH',
      body: JSON.stringify({ read: true }),
    });
  }

  // ============================================================================
  // Decisions
  // ============================================================================

  async getDecisions(
    q?: string,
    status?: string,
    threadId?: string,
    limit: number = 20
  ): Promise<DecisionsResponse> {
    const params = new URLSearchParams({ limit: String(limit) });

    if (q) params.append('q', q);
    if (status) params.append('status', status);
    if (threadId) params.append('thread_id', threadId);

    return this.request<DecisionsResponse>(`/api/decisions?${params}`);
  }

  async getDecision(id: string): Promise<Decision> {
    return this.request<Decision>(`/api/decisions/${id}`);
  }

  async approveDecision(id: string, comment?: string): Promise<void> {
    return this.request<void>(`/api/decisions/${id}/approve`, {
      method: 'POST',
      body: JSON.stringify({ comment }),
    });
  }

  async rejectDecision(id: string, reason: string): Promise<void> {
    return this.request<void>(`/api/decisions/${id}/reject`, {
      method: 'POST',
      body: JSON.stringify({ reason }),
    });
  }

  // ============================================================================
  // Artifacts
  // ============================================================================

  async getArtifacts(
    sharedBy?: string,
    limit: number = 50
  ): Promise<ArtifactsResponse> {
    const params = new URLSearchParams({ limit: String(limit) });

    if (sharedBy) {
      params.append('shared_by', sharedBy);
    }

    return this.request<ArtifactsResponse>(`/api/artifacts?${params}`);
  }

  async getArtifactContent(path: string): Promise<Blob> {
    const url = `${this.baseUrl}/api/artifacts/${encodeURIComponent(path)}`;
    const response = await fetch(url);

    if (!response.ok) {
      throw new Error(`Failed to fetch artifact: ${response.statusText}`);
    }

    return response.blob();
  }

  // ============================================================================
  // Merlin Actions
  // ============================================================================

  async injectMessage(inject: InjectRequest): Promise<void> {
    await this.request<void>('/api/inject', {
      method: 'POST',
      body: JSON.stringify(inject),
    });
  }

  async annotate(annotate: AnnotateRequest): Promise<void> {
    await this.request<void>('/api/annotate', {
      method: 'POST',
      body: JSON.stringify(annotate),
    });
  }

  async getConfig(): Promise<ConfigResponse> {
    return this.request<ConfigResponse>('/api/config');
  }

  async updateConfig(config: UpdateConfigRequest): Promise<ConfigResponse> {
    return this.request<ConfigResponse>('/api/config', {
      method: 'PATCH',
      body: JSON.stringify(config),
    });
  }

  // ============================================================================
  // Search
  // ============================================================================

  async search(
    query: string,
    type: 'messages' | 'decisions' | 'all' = 'all',
    limit: number = 20
  ): Promise<SearchResponse> {
    const params = new URLSearchParams({
      q: query,
      type,
      limit: String(limit),
    });

    return this.request<SearchResponse>(`/api/search?${params}`);
  }
}

// Singleton instance
export const api = new ApiClient();
