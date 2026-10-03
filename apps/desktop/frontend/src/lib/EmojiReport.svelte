<script lang="ts">
  // Emoji search results as a grid; clicking an emoji copies it.
  type Emoji = { emoji: string; name: string; shortcode?: string | null };
  let { value, copy }: { value: unknown; mime?: string; copy: (text: string) => void } = $props();
  const report = $derived((value && typeof value === 'object' ? value : {}) as { emojis?: Emoji[]; count?: number; shown?: number });
  const emojis = $derived(Array.isArray(report.emojis) ? report.emojis.filter((item) => typeof item?.emoji === 'string') : []);
  let last = $state('');

  function pick(item: Emoji): void {
    copy(item.emoji);
    last = item.name;
  }
</script>

{#if emojis.length}
  <div class="emoji-grid" role="group" aria-label="Emoji">
    {#each emojis as item (item.emoji)}
      <button type="button" title="{item.name}{item.shortcode ? ` ${item.shortcode}` : ''}" aria-label="Copy {item.name}" onclick={() => pick(item)}>{item.emoji}</button>
    {/each}
  </div>
  <p class="emoji-note" aria-live="polite">{last ? `Copied ${last}` : 'Click an emoji to copy it.'}{(report.count ?? 0) > emojis.length ? ` Showing ${emojis.length} of ${report.count}; search to narrow down.` : ''}</p>
{:else}
  <p class="emoji-note">No emoji match that search.</p>
{/if}

<style>
  .emoji-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(42px, 1fr)); gap: 4px; max-height: 340px; overflow: auto; }
  .emoji-grid button { aspect-ratio: 1; display: grid; place-items: center; font-size: 24px; line-height: 1; border: 0; border-radius: 8px; background: transparent; cursor: pointer; }
  .emoji-grid button:hover, .emoji-grid button:focus-visible { background: var(--surface-2); }
  .emoji-note { margin: 8px 0 0; font-size: 12px; color: var(--ink-3); }
</style>
