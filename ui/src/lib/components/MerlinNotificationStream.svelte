<script lang="ts">
  import { merlinNotifications } from '$stores/merlinNotifications';
  import type { MerlinNotificationUI } from '$lib/types/notifications';

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

  // Watch for connection changes
  $effect(() => {
    if (onConnectionChange) {
      onConnectionChange(connected);
    }
  });

  // Ensure connection on mount
  $effect(() => {
    if (!connected) {
      merlinNotifications.connect();
    }
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
