<script lang="ts">
  import { merlinNotifications } from '$stores/merlinNotifications';
  import type { Decision, DecisionStatus } from '$lib/types';
  
  interface Props {
    decision: Decision;
  }
  
  let { decision }: Props = $props();
  
  let showReasonTextarea = $state<'approve' | 'reject' | null>(null);
  let reason = $state('');
  let submitting = $state(false);
  const maxReasonChars = 500;
  
  function handleApproveClick() {
    if (decision.status === 'approved') return;
    showReasonTextarea = showReasonTextarea === 'approve' ? null : 'approve';
    reason = '';
  }
  
  function handleRejectClick() {
    if (decision.status === 'rejected') return;
    showReasonTextarea = showReasonTextarea === 'reject' ? null : 'reject';
    reason = '';
  }
  
  function submitApproval() {
    if (!decision.decision_id || submitting) return;
    
    submitting = true;
    
    const success = merlinNotifications.sendIntervention({
      action: 'approve_decision',  // Backend expects snake_case
      decisionId: decision.decision_id,
      reason: reason.trim() || undefined
    });
    
    if (success) {
      merlinNotifications.showToast({
        type: 'success',
        message: 'Decision approved',
        duration: 3000
      });
    } else {
      submitting = false;
    }
  }
  
  function submitRejection() {
    if (!decision.decision_id || submitting) return;
    
    submitting = true;
    
    const success = merlinNotifications.sendIntervention({
      action: 'reject_decision',  // Backend expects snake_case
      decisionId: decision.decision_id,
      reason: reason.trim() || undefined
    });
    
    if (success) {
      merlinNotifications.showToast({
        type: 'success',
        message: 'Decision rejected',
        duration: 3000
      });
    } else {
      submitting = false;
    }
  }
  
  function cancelAction() {
    showReasonTextarea = null;
    reason = '';
  }
  
  function getStatusColor(status: DecisionStatus): string {
    switch (status) {
      case 'pending':
        return 'bg-yellow-100 text-yellow-800';
      case 'approved':
        return 'bg-green-100 text-green-800';
      case 'rejected':
        return 'bg-red-100 text-red-800';
      default:
        return 'bg-gray-100 text-gray-800';
    }
  }
  
  function getStatusIcon(status: DecisionStatus): string {
    switch (status) {
      case 'pending':
        return '⏳';
      case 'approved':
        return '✅';
      case 'rejected':
        return '❌';
      default:
        return '❓';
    }
  }
</script>

<div class="decision-actions">
  <!-- Status Badge -->
  <div class="flex items-center gap-2 mb-3">
    <span class="px-2 py-1 rounded-full text-xs font-medium {getStatusColor(decision.status)}">
      {getStatusIcon(decision.status)} {decision.status.toUpperCase()}
    </span>
  </div>
  
  {#if decision.status === 'pending'}
    <!-- Action Buttons (only for pending decisions) -->
    <div class="flex gap-2">
      <button
        onclick={handleApproveClick}
        disabled={submitting}
        class="flex-1 px-3 py-2 text-green-700 bg-green-50 hover:bg-green-100 border border-green-200 rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
        title="Approve this decision"
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M5 13l4 4L19 7"
          />
        </svg>
        Approve
      </button>
      
      <button
        onclick={handleRejectClick}
        disabled={submitting}
        class="flex-1 px-3 py-2 text-red-700 bg-red-50 hover:bg-red-100 border border-red-200 rounded-md transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
        title="Reject this decision"
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M6 18L18 6M6 6l12 12"
          />
        </svg>
        Reject
      </button>
    </div>
    
    <!-- Reason Textarea (shown when approve/reject clicked) -->
    {#if showReasonTextarea}
      <div class="mt-3 p-3 bg-gray-50 rounded-md border border-gray-200">
        <label for="reason-textarea" class="block text-sm font-medium text-gray-700 mb-2">
          Reason (optional)
        </label>
        <textarea
          id="reason-textarea"
          bind:value={reason}
          disabled={submitting}
          class="w-full px-2 py-1 text-sm border border-gray-300 rounded focus:outline-none focus:ring-1 focus:ring-blue-500 resize-none"
          rows="2"
          placeholder="Why are you {showReasonTextarea}ing this decision? (optional)"
          maxlength={maxReasonChars}
        ></textarea>
        <div class="flex justify-between items-center mt-2">
          <span class="text-xs text-gray-500">
            {reason.length} / {maxReasonChars}
          </span>
          <div class="flex gap-2">
            <button
              onclick={cancelAction}
              disabled={submitting}
              class="px-2 py-1 text-xs text-gray-700 bg-gray-200 hover:bg-gray-300 rounded transition-colors disabled:opacity-50"
            >
              Cancel
            </button>
            {#if showReasonTextarea === 'approve'}
              <button
                onclick={submitApproval}
                disabled={submitting}
                class="px-2 py-1 text-xs text-white bg-green-600 hover:bg-green-700 rounded transition-colors disabled:opacity-50"
              >
                {#if submitting}
                  Approving...
                {:else}
                  Confirm Approve
                {/if}
              </button>
            {:else}
              <button
                onclick={submitRejection}
                disabled={submitting}
                class="px-2 py-1 text-xs text-white bg-red-600 hover:bg-red-700 rounded transition-colors disabled:opacity-50"
              >
                {#if submitting}
                  Rejecting...
                {:else}
                  Confirm Reject
                {/if}
              </button>
            {/if}
          </div>
        </div>
      </div>
    {/if}
  {:else if decision.status === 'approved'}
    <!-- Approved State -->
    <div class="text-sm text-green-700 flex items-center gap-1">
      <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
          d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
        />
      </svg>
      Approved by Merlin
    </div>
  {:else if decision.status === 'rejected'}
    <!-- Rejected State -->
    <div class="text-sm text-red-700 flex items-center gap-1">
      <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
          d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z"
        />
      </svg>
      Rejected by Merlin
    </div>
  {/if}
</div>
