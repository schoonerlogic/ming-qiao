<script lang="ts">
  import type { Message, Priority } from '$lib/types';

  interface Props {
    message: Message;
  }

  let { message }: Props = $props();

  function getPriorityBadgeClass(priority: Priority): string {
    const base = 'px-2 py-1 rounded-full text-xs font-medium ';
    switch (priority) {
      case 'low':
        return base + 'bg-gray-100 text-gray-700';
      case 'normal':
        return base + 'bg-blue-100 text-blue-700';
      case 'high':
        return base + 'bg-orange-100 text-orange-700';
      case 'critical':
        return base + 'bg-red-100 text-red-700';
    }
  }

  function formatDate(dateStr: string): string {
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMs / 3600000);
    const diffDays = Math.floor(diffMs / 86400000);

    if (diffMins < 1) return 'just now';
    if (diffMins < 60) return `${diffMins}m ago`;
    if (diffHours < 24) return `${diffHours}h ago`;
    if (diffDays < 7) return `${diffDays}d ago`;
    return date.toLocaleDateString();
  }

  function getInitials(name: string): string {
    return name
      .split(' ')
      .map((n) => n[0])
      .join('')
      .toUpperCase()
      .slice(0, 2);
  }

  function getAvatarColor(name: string): string {
    const colors = [
      'bg-red-500',
      'bg-blue-500',
      'bg-green-500',
      'bg-yellow-500',
      'bg-purple-500',
      'bg-pink-500',
      'bg-indigo-500',
    ];
    const index = name.split('').reduce((acc, char) => acc + char.charCodeAt(0), 0);
    return colors[index % colors.length];
  }
</script>

<div class="message bg-white border border-gray-200 rounded-lg p-4 hover:shadow-sm transition-shadow">
  <div class="flex items-start gap-3">
    <!-- Avatar -->
    <div class="flex-shrink-0">
      <div
        class="w-10 h-10 rounded-full {getAvatarColor(message.from_agent)} flex items-center justify-center text-white font-semibold"
      >
        {getInitials(message.from_agent)}
      </div>
    </div>

    <!-- Message Content -->
    <div class="flex-1 min-w-0">
      <div class="flex items-center gap-2 mb-1">
        <span class="font-semibold text-gray-900">{message.from_agent}</span>
        <span class="text-gray-400">→</span>
        <span class="text-gray-700">{message.to_agent}</span>
        <span class={getPriorityBadgeClass(message.priority)}>{message.priority}</span>
        <span class="text-xs text-gray-500 ml-auto">{formatDate(message.sent_at)}</span>
      </div>

      {#if message.subject}
        <p class="font-medium text-gray-900 mb-1">{message.subject}</p>
      {/if}

      <p class="text-gray-700 whitespace-pre-wrap break-words">{message.content}</p>

      {#if message.artifact_refs && message.artifact_refs.length > 0}
        <div class="mt-2 space-y-1">
          <p class="text-sm font-medium text-gray-700">Attachments:</p>
          {#each message.artifact_refs as ref}
            <div
              class="flex items-center gap-2 text-sm text-blue-600 hover:text-blue-800 cursor-pointer"
            >
              <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M15.172 7l-6.586 6.586a2 2 0 102.828 2.828l6.414-6.586a4 4 0 00-5.656-5.656l-6.415 6.585a6 6 0 108.486 8.486L20.5 13"
                />
              </svg>
              {ref.path}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>
