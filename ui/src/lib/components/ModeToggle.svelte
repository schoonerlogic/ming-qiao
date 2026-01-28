<script lang="ts">
  import { configStore, setMode, updateLocalMode } from '$stores/config';
  import { merlinNotifications } from '$stores/merlinNotifications';
  import type { ObservationMode } from '$lib/types';
  
  let loading = $state(false);
  let previousMode = $state<ObservationMode | null>(null);
  
  const modes: { value: ObservationMode; label: string; description: string }[] = [
    { value: 'passive', label: 'Passive', description: 'Observe only, no notifications' },
    { value: 'advisory', label: 'Advisory', description: 'Notify on important events' },
    { value: 'gated', label: 'Gated', description: 'Require approval for actions' },
  ];
  
  function getModeColor(mode: ObservationMode): string {
    switch (mode) {
      case 'passive':
        return 'bg-gray-500';
      case 'advisory':
        return 'bg-blue-500';
      case 'gated':
        return 'bg-purple-500';
    }
  }
  
  async function handleModeChange(mode: ObservationMode) {
    if (loading || mode === configStore.mode) return;
    
    loading = true;
    previousMode = configStore.mode;
    
    // Optimistic update
    const oldMode = configStore.mode;
    updateLocalMode(mode);
    
    try {
      // Send intervention via WebSocket
      const success = merlinNotifications.sendIntervention({
        action: 'set_mode',  // Backend expects snake_case
        mode
      });
      
      if (success) {
        // Show success toast
        merlinNotifications.showToast({
          type: 'success',
          message: `Mode changed to ${mode}`,
          duration: 3000
        });
        
        // Also update via API to keep in sync
        await setMode(mode);
      } else {
        // Revert on failure
        updateLocalMode(oldMode);
        merlinNotifications.showToast({
          type: 'error',
          message: 'Failed to change mode: WebSocket disconnected',
          duration: 10000
        });
      }
    } catch (e) {
      // Revert on error
      updateLocalMode(oldMode);
      console.error('Error changing mode:', e);
      merlinNotifications.showToast({
        type: 'error',
        message: `Failed to change mode: ${e instanceof Error ? e.message : 'Unknown error'}`,
        duration: 10000
      });
    } finally {
      loading = false;
      previousMode = null;
    }
  }
</script>

<div class="mode-toggle bg-white border border-gray-200 rounded-lg p-4">
  <div class="flex items-center justify-between mb-3">
    <div>
      <h3 class="font-semibold text-gray-900">Observation Mode</h3>
      <p class="text-sm text-gray-600">Current: {configStore.mode}</p>
    </div>
    <div class="w-3 h-3 rounded-full {getModeColor(configStore.mode)}"></div>
  </div>

  <div class="space-y-2">
    {#each modes as mode}
      <button
        class="w-full flex items-start gap-3 p-3 rounded-md border {configStore.mode === mode.value
          ? 'border-blue-500 bg-blue-50'
          : 'border-gray-200 hover:bg-gray-50'} transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        disabled={loading}
        onclick={() => handleModeChange(mode.value)}
      >
        <div class="flex-1 text-left">
          <div class="flex items-center gap-2">
            <span class="font-medium text-gray-900">{mode.label}</span>
            {#if configStore.mode === mode.value}
              <span class="text-xs text-blue-600">(active)</span>
            {/if}
          </div>
          <p class="text-sm text-gray-600">{mode.description}</p>
        </div>
        <div class="w-4 h-4 rounded-full border-2 {configStore.mode === mode.value
          ? 'border-blue-500 bg-blue-500'
          : 'border-gray-300'} flex items-center justify-center">
          {#if configStore.mode === mode.value}
            <svg class="w-3 h-3 text-white" fill="currentColor" viewBox="0 0 20 20">
              <path
                fill-rule="evenodd"
                d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z"
                clip-rule="evenodd"
              />
            </svg>
          {/if}
        </div>
      </button>
    {/each}
  </div>

  {#if loading}
    <div class="mt-3 text-center text-sm text-gray-500">
      <span class="inline-block animate-spin mr-2">⟳</span>
      Changing mode...
    </div>
  {/if}
</div>
