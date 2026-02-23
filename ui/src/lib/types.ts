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

export type MessageIntent = 'discuss' | 'request' | 'inform';

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
  intent?: MessageIntent;
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
  id: string;
  thread_id?: string; // Optional alias for compatibility
  subject: string;
  participants: string[];
  status: ThreadStatus;
  started_at?: string;
  created_at: string; // Backend uses created_at
  last_message_at?: string;
  message_count: number;
  decision_count?: number;
  unread_count?: number;
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
// WebSocket Types - Merlin Notifications
// ============================================================================

export type WSMessage =
  | WSConnected
  | WSPriorityAlert
  | WSKeywordDetected
  | WSDecisionReview
  | WSActionBlocked
  | WSStatusUpdate
  | WSError;

export interface WSConnected {
  type: 'connected';
  message: string;
  mode: ObservationMode;
}

export interface WSPriorityAlert {
  type: 'priority_alert';
  event: EventEnvelope;
  reason: string;
}

export interface WSKeywordDetected {
  type: 'keyword_detected';
  event: EventEnvelope;
  keyword: string;
}

export interface WSDecisionReview {
  type: 'decision_review';
  event: EventEnvelope;
  decision_type: string;
}

export interface WSActionBlocked {
  type: 'action_blocked';
  event: EventEnvelope;
  reason: string;
}

export interface WSStatusUpdate {
  type: 'status_update';
  message: string;
  timestamp: string;
}

export interface WSError {
  type: 'error';
  message: string;
}

// Backend event envelope (from Rust)
export interface EventEnvelope {
  event_id: string;
  event_type: string;
  agent_id: string;
  timestamp: string;
  payload: any;
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
