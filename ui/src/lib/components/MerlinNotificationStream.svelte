<script lang="ts">
  import { merlinNotifications } from '$stores/merlinNotifications';
  import type { MerlinNotificationUI } from '$lib/types/notifications';
  import { onMount } from 'svelte';

  /**
   * MerlinNotificationStream - Invisible component that manages the notification stream
   * This should be placed in the app layout to maintain a persistent connection
   */

  interface Props {
    onNotificationReceived?: (notification: MerlinNotificationUI) => void;
    onConnectionChange?: (connected: boolean) => void;
  }

  let { onNotificationReceived, onConnectionChange }: Props = $props();

  // Connection status
  let connected = $derived(merlinNotifications.connected);
  let connectionError = $derived(merlinNotifications.connectionError);

  // Track if we've already initiated connection
  let hasInitialized = $state(false);

  // Watch for connection changes
  $effect(() => {
    if (onConnectionChange) {
      onConnectionChange(connected);
    }
  });

  // Initialize connection ONCE on mount
  onMount(() => {
    console.log('[MerlinNotificationStream] Component mounted, initiating connection...');
    merlinNotifications.connect();
    hasInitialized = true;
  });
</script>

<!-- This component is invisible - it only manages the notification stream -->
<div class="hidden" data-connected={connected} data-error={connectionError || ''}>
  {#if connectionError}
    <div class="notification-stream-error">
      Notification stream error: {connectionError}
    </div>
  {/if}
</div>

<style>
  .hidden {
    display: none;
  }
</style>
