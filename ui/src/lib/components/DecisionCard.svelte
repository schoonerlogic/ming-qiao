<script lang="ts">
  import type { Decision, DecisionStatus } from '$lib/types';
  import DecisionActions from './DecisionActions.svelte';

  interface Props {
    decision: Decision;
  }

  let { decision }: Props = $props();

  function getStatusBadgeClass(status: DecisionStatus): string {
    const base = 'px-2 py-1 rounded-full text-xs font-medium ';
    switch (status) {
      case 'pending':
        return base + 'bg-yellow-100 text-yellow-800';
      case 'approved':
        return base + 'bg-green-100 text-green-800';
      case 'rejected':
        return base + 'bg-red-100 text-red-800';
      case 'superseded':
        return base + 'bg-gray-100 text-gray-800';
    }
  }

  function formatDate(dateStr: string): string {
    const date = new Date(dateStr);
    return date.toLocaleDateString();
  }
</script>

<div class="decision-card bg-white border border-gray-200 rounded-lg p-4 hover:shadow-sm transition-shadow">
  <div class="flex items-start justify-between mb-3">
    <div class="flex-1">
      <div class="flex items-center gap-2 mb-1">
        <svg class="w-5 h-5 text-gray-700" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
        <h3 class="font-semibold text-gray-900">Decision Required</h3>
        <span class={getStatusBadgeClass(decision.status)}>{decision.status}</span>
      </div>
      <p class="text-gray-900 font-medium">{decision.question}</p>
    </div>
  </div>

  {#if decision.resolution}
    <div class="bg-green-50 border-l-4 border-green-500 p-3 mb-3">
      <p class="font-medium text-green-900">Resolution</p>
      <p class="text-green-800">{decision.resolution}</p>
      {#if decision.rationale}
        <p class="text-sm text-green-700 mt-1">{decision.rationale}</p>
      {/if}
      <div class="flex items-center gap-2 mt-2 text-xs text-green-600">
        <span>Decided by {decision.decided_by}</span>
        {#if decision.decided_at}
          <span>• {formatDate(decision.decided_at)}</span>
        {/if}
      </div>
    </div>
  {/if}

  {#if decision.options && decision.options.length > 0}
    <div class="space-y-2 mb-3">
      <p class="text-sm font-medium text-gray-700">Options:</p>
      {#each decision.options as option}
        <div class="border border-gray-200 rounded p-2">
          <p class="font-medium text-gray-900">{option.label}</p>
          <p class="text-sm text-gray-600">{option.description}</p>
          {#if option.pros.length > 0 || option.cons.length > 0}
            <div class="mt-2 flex gap-4 text-xs">
              {#if option.pros.length > 0}
                <div>
                  <span class="font-medium text-green-700">Pros:</span>
                  <span class="text-gray-600">{option.pros.join(', ')}</span>
                </div>
              {/if}
              {#if option.cons.length > 0}
                <div>
                  <span class="font-medium text-red-700">Cons:</span>
                  <span class="text-gray-600">{option.cons.join(', ')}</span>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <!-- Merlin Decision Actions -->
  <DecisionActions {decision} />
</div>
