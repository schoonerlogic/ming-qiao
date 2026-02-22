<script lang="ts">
  import { api } from '$lib/api';
  import { injectMessage, approveDecision, rejectDecision, setMode } from '$stores/websocket.svelte';
  import { configStore } from '$stores/config.svelte';
  import type { ObservationMode } from '$lib/types';

  // Panel state
  let activeTab = $state<'message' | 'decisions' | 'mode'>('message');
  let threadId = $state<string>('');
  let messageContent = $state<string>('');
  let decisionId = $state<string>('');
  let decisionReason = $state<string>('');
  let selectedMode = $state<ObservationMode>('passive');

  // Loading states
  let injectingMessage = $state(false);
  let processingDecision = $state(false);
  let updatingMode = $state(false);

  // Result messages
  let resultMessage = $state<string>('');
  let resultType = $state<'success' | 'error' | null>(null);

  async function handleInjectMessage() {
    if (!threadId.trim() || !messageContent.trim()) {
      showResult('error', 'Please fill in all fields');
      return;
    }

    injectingMessage = true;
    try {
      // Use HTTP API for injection (more reliable than WebSocket)
      await api.injectMessage({
        thread_id: threadId.trim(),
        content: messageContent.trim(),
        action: 'comment',
      });

      showResult('success', 'Message injected successfully');
      messageContent = '';
      threadId = '';
    } catch (e) {
      showResult('error', 'Failed to inject message: ' + (e instanceof Error ? e.message : 'Unknown error'));
    } finally {
      injectingMessage = false;
    }
  }

  async function handleApproveDecision() {
    if (!decisionId.trim()) {
      showResult('error', 'Please enter a decision ID');
      return;
    }

    processingDecision = true;
    try {
      await api.approveDecision(decisionId.trim(), decisionReason.trim() || undefined);

      showResult('success', 'Decision approved successfully');
      decisionId = '';
      decisionReason = '';
    } catch (e) {
      showResult('error', 'Failed to approve decision: ' + (e instanceof Error ? e.message : 'Unknown error'));
    } finally {
      processingDecision = false;
    }
  }

  async function handleRejectDecision() {
    if (!decisionId.trim()) {
      showResult('error', 'Please enter a decision ID');
      return;
    }

    if (!decisionReason.trim()) {
      showResult('error', 'Please provide a reason for rejection');
      return;
    }

    processingDecision = true;
    try {
      await api.rejectDecision(decisionId.trim(), decisionReason.trim());

      showResult('success', 'Decision rejected successfully');
      decisionId = '';
      decisionReason = '';
    } catch (e) {
      showResult('error', 'Failed to reject decision: ' + (e instanceof Error ? e.message : 'Unknown error'));
    } finally {
      processingDecision = false;
    }
  }

  async function handleSetMode() {
    if (selectedMode === configStore.mode) {
      showResult('error', 'Mode is already set to ' + selectedMode);
      return;
    }

    updatingMode = true;
    try {
      await setMode(selectedMode);

      showResult('success', 'Observation mode changed to ' + selectedMode);
    } catch (e) {
      showResult('error', 'Failed to change mode: ' + (e instanceof Error ? e.message : 'Unknown error'));
    } finally {
      updatingMode = false;
    }
  }

  function showResult(type: 'success' | 'error', message: string) {
    resultType = type;
    resultMessage = message;

    // Auto-clear after 5 seconds
    setTimeout(() => {
      resultMessage = '';
      resultType = null;
    }, 5000);
  }

  function getModeDescription(mode: ObservationMode): string {
    switch (mode) {
      case 'passive':
        return 'Observe only, no notifications';
      case 'advisory':
        return 'Notify on important events';
      case 'gated':
        return 'Require approval for actions';
    }
  }

  function getModeIcon(mode: ObservationMode): string {
    switch (mode) {
      case 'passive':
        return '👁️';
      case 'advisory':
        return '🔔';
      case 'gated':
        return '🔒';
    }
  }
</script>

<div class="intervention-panel">
  <!-- Panel Header -->
  <div class="panel-header">
    <h2>Intervention Panel</h2>
    <p class="panel-description">Inject messages, approve decisions, change mode</p>
  </div>

  <!-- Tabs -->
  <div class="panel-tabs">
    <button
      class="tab-btn {activeTab === 'message' ? 'active' : ''}"
      onclick={() => activeTab = 'message'}
    >
      💬 Message
    </button>
    <button
      class="tab-btn {activeTab === 'decisions' ? 'active' : ''}"
      onclick={() => activeTab = 'decisions'}
    >
      🎯 Decisions
    </button>
    <button
      class="tab-btn {activeTab === 'mode' ? 'active' : ''}"
      onclick={() => activeTab = 'mode'}
    >
      🔧 Mode
    </button>
  </div>

  <!-- Result Message -->
  {#if resultMessage}
    <div class="result-message {resultType}">
      <span class="result-icon">{resultType === 'success' ? '✅' : '⚠️'}</span>
      <span class="result-text">{resultMessage}</span>
      <button class="result-close" onclick={() => { resultMessage = ''; resultType = null; }}>✕</button>
    </div>
  {/if}

  <!-- Tab Content: Message Injection -->
  {#if activeTab === 'message'}
    <div class="tab-content">
      <div class="form-group">
        <label for="thread-id">Thread ID</label>
        <input
          id="thread-id"
          type="text"
          placeholder="Enter thread ID..."
          bind:value={threadId}
          disabled={injectingMessage}
        />
      </div>

      <div class="form-group">
        <label for="message-content">Message</label>
        <textarea
          id="message-content"
          placeholder="Type your message to inject..."
          bind:value={messageContent}
          disabled={injectingMessage}
          rows="5"
        ></textarea>
        <div class="char-count">{messageContent.length} characters</div>
      </div>

      <div class="form-actions">
        <button
          class="action-btn primary"
          onclick={handleInjectMessage}
          disabled={injectingMessage || !threadId.trim() || !messageContent.trim()}
        >
          {injectingMessage ? '⏳ Injecting...' : '💬 Inject Message'}
        </button>
      </div>

      <div class="info-box">
        <span class="info-icon">ℹ️</span>
        <span class="info-text">Messages will be injected as Merlin into the specified thread</span>
      </div>
    </div>
  {/if}

  <!-- Tab Content: Decisions -->
  {#if activeTab === 'decisions'}
    <div class="tab-content">
      <div class="form-group">
        <label for="decision-id">Decision ID</label>
        <input
          id="decision-id"
          type="text"
          placeholder="Enter decision ID..."
          bind:value={decisionId}
          disabled={processingDecision}
        />
      </div>

      <div class="form-group">
        <label for="decision-reason">Reason (required for rejection)</label>
        <textarea
          id="decision-reason"
          placeholder="Provide a reason for approval or rejection..."
          bind:value={decisionReason}
          disabled={processingDecision}
          rows="3"
        ></textarea>
        <div class="char-count">{decisionReason.length} characters</div>
      </div>

      <div class="form-actions">
        <button
          class="action-btn approve"
          onclick={handleApproveDecision}
          disabled={processingDecision || !decisionId.trim()}
        >
          {processingDecision ? '⏳ Processing...' : '✅ Approve'}
        </button>

        <button
          class="action-btn reject"
          onclick={handleRejectDecision}
          disabled={processingDecision || !decisionId.trim() || !decisionReason.trim()}
        >
          {processingDecision ? '⏳ Processing...' : '❌ Reject'}
        </button>
      </div>

      <div class="info-box">
        <span class="info-icon">ℹ️</span>
        <span class="info-text">Find pending decisions in the Decision Log tab</span>
      </div>
    </div>
  {/if}

  <!-- Tab Content: Mode -->
  {#if activeTab === 'mode'}
    <div class="tab-content">
      <div class="mode-status">
        <span class="mode-label">Current Mode:</span>
        <span class="mode-value">{getModeIcon(configStore.mode)} {configStore.mode}</span>
      </div>

      <div class="mode-options">
        {#each ['passive', 'advisory', 'gated'] as mode}
          {@const modeKey = mode as ObservationMode}
          <div
            class="mode-option {selectedMode === modeKey ? 'selected' : ''} {configStore.mode === modeKey ? 'current' : ''}"
            onclick={() => selectedMode = modeKey}
          >
            <div class="mode-option-header">
              <span class="mode-option-icon">{getModeIcon(modeKey)}</span>
              <span class="mode-option-name">{modeKey}</span>
              {#if configStore.mode === modeKey}
                <span class="mode-current-badge">Current</span>
              {/if}
            </div>
            <div class="mode-option-desc">{getModeDescription(modeKey)}</div>
          </div>
        {/each}
      </div>

      <div class="form-actions">
        <button
          class="action-btn primary"
          onclick={handleSetMode}
          disabled={updatingMode || selectedMode === configStore.mode}
        >
          {updatingMode ? '⏳ Changing...' : '🔧 Set Mode'}
        </button>
      </div>

      <div class="info-box">
        <span class="info-icon">ℹ️</span>
        <span class="info-text">Changing mode affects which notifications you receive</span>
      </div>
    </div>
  {/if}
</div>

<style>
  .intervention-panel {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    background: white;
    border-radius: 8px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.1);
    overflow: hidden;
  }

  /* Header */
  .panel-header {
    padding: 1.5rem;
    border-bottom: 1px solid #e5e7eb;
    background: #f9fafb;
  }

  .panel-header h2 {
    margin: 0 0 0.25rem 0;
    font-size: 1.25rem;
    font-weight: 600;
    color: #111827;
  }

  .panel-description {
    margin: 0;
    font-size: 0.875rem;
    color: #6b7280;
  }

  /* Tabs */
  .panel-tabs {
    display: flex;
    border-bottom: 1px solid #e5e7eb;
  }

  .tab-btn {
    flex: 1;
    padding: 0.75rem 1rem;
    border: none;
    background: white;
    cursor: pointer;
    font-size: 0.875rem;
    font-weight: 500;
    color: #6b7280;
    transition: all 0.2s;
    border-bottom: 2px solid transparent;
  }

  .tab-btn:hover {
    background: #f9fafb;
    color: #111827;
  }

  .tab-btn.active {
    color: #3b82f6;
    border-bottom-color: #3b82f6;
    background: #eff6ff;
  }

  /* Result Message */
  .result-message {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 1rem 1.5rem;
    margin: 0 1rem;
    border-radius: 6px;
  }

  .result-message.success {
    background: #d1fae5;
    color: #065f46;
    border: 1px solid #10b981;
  }

  .result-message.error {
    background: #fee2e2;
    color: #991b1b;
    border: 1px solid #ef4444;
  }

  .result-icon {
    font-size: 1.25rem;
  }

  .result-text {
    flex: 1;
    font-size: 0.875rem;
    font-weight: 500;
  }

  .result-close {
    background: none;
    border: none;
    cursor: pointer;
    font-size: 1rem;
    opacity: 0.7;
    transition: opacity 0.2s;
  }

  .result-close:hover {
    opacity: 1;
  }

  /* Tab Content */
  .tab-content {
    padding: 1.5rem;
  }

  .form-group {
    margin-bottom: 1rem;
  }

  .form-group label {
    display: block;
    font-size: 0.875rem;
    font-weight: 500;
    color: #374151;
    margin-bottom: 0.5rem;
  }

  .form-group input,
  .form-group textarea {
    width: 100%;
    padding: 0.625rem;
    border: 1px solid #d1d5db;
    border-radius: 6px;
    font-size: 0.875rem;
    font-family: inherit;
  }

  .form-group input:focus,
  .form-group textarea:focus {
    outline: none;
    border-color: #3b82f6;
    box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.1);
  }

  .form-group input:disabled,
  .form-group textarea:disabled {
    background: #f3f4f6;
    cursor: not-allowed;
    opacity: 0.6;
  }

  .char-count {
    text-align: right;
    font-size: 0.75rem;
    color: #9ca3af;
    margin-top: 0.25rem;
  }

  .form-actions {
    display: flex;
    gap: 0.75rem;
    margin-top: 1.5rem;
  }

  .action-btn {
    flex: 1;
    padding: 0.625rem 1rem;
    border: none;
    border-radius: 6px;
    font-size: 0.875rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s;
  }

  .action-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .action-btn.primary {
    background: #3b82f6;
    color: white;
  }

  .action-btn.primary:hover:not(:disabled) {
    background: #2563eb;
  }

  .action-btn.approve {
    background: #10b981;
    color: white;
  }

  .action-btn.approve:hover:not(:disabled) {
    background: #059669;
  }

  .action-btn.reject {
    background: #ef4444;
    color: white;
  }

  .action-btn.reject:hover:not(:disabled) {
    background: #dc2626;
  }

  .info-box {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem;
    background: #eff6ff;
    border-radius: 6px;
    margin-top: 1rem;
  }

  .info-icon {
    font-size: 1rem;
  }

  .info-text {
    font-size: 0.875rem;
    color: #1e40af;
  }

  /* Mode Status */
  .mode-status {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem;
    background: #f9fafb;
    border-radius: 6px;
    margin-bottom: 1rem;
  }

  .mode-label {
    font-size: 0.875rem;
    font-weight: 500;
    color: #6b7280;
  }

  .mode-value {
    font-size: 0.875rem;
    font-weight: 600;
    color: #111827;
  }

  /* Mode Options */
  .mode-options {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    margin-bottom: 1.5rem;
  }

  .mode-option {
    padding: 1rem;
    border: 2px solid #e5e7eb;
    border-radius: 8px;
    cursor: pointer;
    transition: all 0.2s;
  }

  .mode-option:hover {
    border-color: #d1d5db;
    background: #f9fafb;
  }

  .mode-option.selected {
    border-color: #3b82f6;
    background: #eff6ff;
  }

  .mode-option.current {
    border-color: #10b981;
  }

  .mode-option-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.25rem;
  }

  .mode-option-icon {
    font-size: 1.25rem;
  }

  .mode-option-name {
    font-size: 1rem;
    font-weight: 600;
    color: #111827;
    text-transform: capitalize;
  }

  .mode-current-badge {
    margin-left: auto;
    padding: 0.25rem 0.5rem;
    background: #10b981;
    color: white;
    font-size: 0.75rem;
    font-weight: 500;
    border-radius: 9999px;
  }

  .mode-option-desc {
    font-size: 0.875rem;
    color: #6b7280;
  }
</style>
