<script lang="ts">
  import { api } from '$lib/api';
  import type { SearchResponse, SearchResult } from '$lib/types';

  let query = $state('');
  let results = $state<SearchResult[]>([]);
  let loading = $state(false);
  let showResults = $state(false);
  let total = $state(0);

  async function handleSearch() {
    if (!query.trim() || loading) return;

    loading = true;
    try {
      const response = await api.search(query.trim(), 'all', 10);
      results = response.results;
      total = response.total;
      showResults = true;
    } catch (e) {
      console.error('Error searching:', e);
      alert('Search failed: ' + (e instanceof Error ? e.message : 'Unknown error'));
    } finally {
      loading = false;
    }
  }

  async function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      await handleSearch();
    } else if (event.key === 'Escape') {
      showResults = false;
      query = '';
    }
  }

  function getResultIcon(type: string): string {
    return type === 'decision' ? '✓' : '💬';
  }

  function getResultColor(type: string): string {
    return type === 'decision' ? 'text-green-600' : 'text-blue-600';
  }
</script>

<div class="search-bar relative">
  <div class="relative">
    <input
      bind:value={query}
      onkeydown={handleKeydown}
      disabled={loading}
      type="text"
      placeholder="Search messages and decisions..."
      class="w-full px-4 py-2 pl-10 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50 disabled:cursor-not-allowed"
    />
    <svg
      class="w-5 h-5 text-gray-400 absolute left-3 top-2.5"
      fill="none"
      stroke="currentColor"
      viewBox="0 0 24 24"
    >
      <path
        stroke-linecap="round"
        stroke-linejoin="round"
        stroke-width="2"
        d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
      />
    </svg>
    {#if loading}
      <div class="absolute right-3 top-2.5">
        <div class="animate-spin rounded-full h-5 w-5 border-b-2 border-blue-500"></div>
      </div>
    {:else if query}
      <button
        onclick={() => {
          query = '';
          showResults = false;
        }}
        class="absolute right-3 top-2.5 text-gray-400 hover:text-gray-600"
      >
        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
        </svg>
      </button>
    {/if}
  </div>

  {#if showResults && (results.length > 0 || total === 0)}
    <div class="absolute z-10 w-full mt-2 bg-white border border-gray-200 rounded-md shadow-lg max-h-96 overflow-y-auto">
      {#if results.length === 0}
        <div class="p-4 text-center text-gray-500">
          <p>No results found for "{query}"</p>
          <p class="text-sm">Try different keywords</p>
        </div>
      {:else}
        <div class="p-2 border-b border-gray-100">
          <p class="text-sm text-gray-600">
            Found {total} {total === 1 ? 'result' : 'results'} for "{query}"
          </p>
        </div>
        <div class="divide-y divide-gray-100">
          {#each results as result}
            <div
              class="p-3 hover:bg-gray-50 cursor-pointer transition-colors"
              onclick={() => {
                // Navigate to result (implement routing later)
                console.log('Navigate to:', result.type, result.id);
                showResults = false;
              }}
            >
              <div class="flex items-start gap-2">
                <span class="text-lg {getResultColor(result.type)}">{getResultIcon(result.type)}
                </span>
                <div class="flex-1 min-w-0">
                  <p class="text-sm font-medium text-gray-900 capitalize">{result.type}</p>
                  <p class="text-sm text-gray-600 line-clamp-2">{result.snippet}</p>
                  <div class="flex items-center gap-2 mt-1 text-xs text-gray-500">
                    <span>Score: {(result.score * 100).toFixed(0)}%</span>
                    <span>ID: {result.id.slice(0, 8)}...</span>
                  </div>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>
