/**
 * TypeScript types for Ming-Qiao UI
 * Based on HTTP API specification and event schemas
 */

// ============================================================================
// Enums
// ============================================================================

export type Priority = 'low' | 'normal' | 'high' | 'critical';

export type ThreadStatus = 'active' | 'paused' | 'resolved' | 'archived';

export type DecisionStatus = 'pending' | 'approved' | 'rejected' | 'superseded';

export type AgentStatus = 'available' | 'working' | 'blocked' | 'offline';

export type ObservationMode = 'passive' | 'advisory' | 'gated';

export type InjectAction = 'comment' | 'pause' | 'redirect' | 'approve' | 'reject';

export type AnnotationTarget = 'thread' | 'decision' | 'message';

// ============================================================================
// Core Types
// ============================================================================

export interface Message {
  message_id: string;
  thread_id: string;
  from_agent: string;
  to_agent: string;
  subject?: string;
  content: string;
  priority: Priority;
  sent_at: string;
  read_at?: string;
  artifact_refs?: ArtifactRef[];
  context_refs?: any[];
}

export interface ArtifactRef {
  artifact_id: string;
  path: string;
  sha256: string;
}

export interface Thread {
  id: string;  // Backend returns 'id', not 'thread_id'
  subject: string;
  participants: string[];
  status: ThreadStatus;
  created_at: string;  // Backend returns 'created_at', not 'started_at'
  last_message_at: string;
  message_count: number;
  decision_count: number;
  unread_count: number;
}

export interface ThreadDetail {
  thread_id: string;
  subject: string;
  participants: string[];
  status: ThreadStatus;
  started_at: string;
  messages: Message[];
  decisions: Decision[];
}

export interface Decision {
  decision_id: string;
  thread_id: string;
  question: string;
  resolution?: string;
  rationale?: string;
  decided_by?: string;
  approved_by?: string;
  status: DecisionStatus;
  decided_at?: string;
  options?: DecisionOption[];
}

export interface DecisionOption {
  label: string;
  description: string;
  pros: string[];
  cons: string[];
}

export interface Artifact {
  artifact_id: string;
  path: string;
  shared_by: string;
  sha256: string;
  bytes: number;
  content_type: string;
  description?: string;
  shared_at: string;
  thread_id?: string;
}

export interface Agent {
  agent_id: string;
  display_name: string;
  status: AgentStatus;
  current_task?: string;
  last_seen: string;
}

export interface Annotation {
  annotation_id: string;
  target_type: AnnotationTarget;
  target_id: string;
  content: string;
  created_by: string;
  created_at: string;
}

// ============================================================================
// API Response Types
// ============================================================================

export interface InboxResponse {
  agent: string;
  messages: Message[];
  unread_count: number;
  total_count: number;
}

export interface ThreadsResponse {
  threads: Thread[];
  total: number;
}

export interface DecisionsResponse {
  decisions: Decision[];
  total: number;
}

export interface ArtifactsResponse {
  artifacts: Artifact[];
}

export interface SearchResponse {
  query: string;
  results: SearchResult[];
  total: number;
}

export interface SearchResult {
  type: 'decision' | 'message';
  id: string;
  snippet: string;
  score: number;
}

export interface ConfigResponse {
  mode: ObservationMode;
  notify_on: {
    priority: Priority[];
    keywords: string[];
    decision_type: string[];
  };
}

// ============================================================================
// WebSocket Types
// ============================================================================

export type WSMessage =
  | WSConnected
  | WSMessageEvent
  | WSDecisionPending
  | WSThreadStatus
  | WSAgentTyping
  | WSModeChanged;

export interface WSConnected {
  type: 'connected';
  mode: ObservationMode;
  unread_count: number;
}

export interface WSMessageEvent {
  type: 'message';
  thread_id: string;
  message: Message;
}

export interface WSDecisionPending {
  type: 'decision_pending';
  decision: Decision;
}

export interface WSThreadStatus {
  type: 'thread_status';
  thread_id: string;
  status: ThreadStatus;
}

export interface WSAgentTyping {
  type: 'agent_typing';
  agent: string;
  thread_id: string;
}

export interface WSModeChanged {
  type: 'mode_changed';
  old_mode: ObservationMode;
  new_mode: ObservationMode;
}

// ============================================================================
// Request Types
// ============================================================================

export interface ReplyRequest {
  from_agent: string;
  content: string;
  priority: Priority;
  artifact_refs?: ArtifactRef[];
}

export interface CreateThreadRequest {
  subject: string;
  from_agent: string;
  to_agent: string;
  content: string;
  priority: Priority;
}

export interface InjectRequest {
  thread_id: string;
  content: string;
  action: InjectAction;
}

export interface AnnotateRequest {
  target_type: AnnotationTarget;
  target_id: string;
  content: string;
}

export interface UpdateConfigRequest {
  mode?: ObservationMode;
  notify_on?: {
    priority?: Priority[];
    keywords?: string[];
    decision_type?: string[];
  };
}
