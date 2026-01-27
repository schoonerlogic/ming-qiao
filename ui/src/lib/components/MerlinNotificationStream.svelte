<script lang="ts">
  import { merlinNotifications } from '$stores/merlinNotifications';
  import type { MerlinNotificationUI } from '$lib/types/notifications';

  /**
   * MerlinNotificationStream - Invisible component that manages the notification stream
   * This should be placed in the app layout to maintain a persistent connection
   */

  // Connection status
  let connected = $derived(merlinNotifications.connected);
  let connectionError = $derived(merlinNotifications.connectionError);

  // Subscribe to new notifications
  let recentNotifications = $state<MerlinNotificationUI[]>([]);

  $effect(() => {
    const unsubscribe = merlinNotifications.onNotification((notification) => {
      // Keep track of recent notifications for debugging/display
      recentNotifications = [notification, ...recentNotifications].slice(0, 5);
    });

    return unsubscribe;
  });

  // Expose notification state to parent components via callbacks
  export let onNotificationReceived: ((notification: MerlinNotificationUI) => void) | undefined = undefined;
  export let onConnectionChange: ((connected: boolean) => void) | undefined = undefined;

  // Watch for connection changes
  $effect(() => {
    if (onConnectionChange) {
      onConnectionChange(connected);
    }
  });

  // Forward notifications to parent callback
  $effect(() => {
    if (onNotificationReceived) {
      const unsubscribe = merlinNotifications.onNotification(onNotificationReceived);
      return unsubscribe;
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
