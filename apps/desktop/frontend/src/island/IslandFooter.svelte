<script lang="ts">
  import { useBox } from '../box.svelte';

  const box = useBox();
  // What Escape does right now (BR-0, BR-5): the footer names it and its button does the same.
  const escapeAction = $derived(
    box.onboardingVisible ? 'Close' : box.selectedTool ? 'Back' : box.query.trim() ? 'Clear' : 'Close',
  );
  function escape(): void {
    if (escapeAction === 'Back') box.backFromTool();
    else if (escapeAction === 'Clear') {
      box.query = '';
      box.searchInput?.focus();
    } else void box.dismissIsland();
  }
</script>
<footer class="hint-bar island-footer">
  {#if box.onboardingVisible}
    <span class="hint"><kbd class="key-chip">Tab</kbd>Next</span>
    <span class="hint"><kbd class="key-chip">↵</kbd>Choose</span>
  {:else if box.selectedTool}
    <span class="hint"><kbd class="key-chip">{box.commandKey}</kbd><kbd class="key-chip">↵</kbd>Run</span>
    <span class="hint"><kbd class="key-chip">Tab</kbd>Next field</span>
  {:else}
    <span class="hint"><kbd class="key-chip">↑</kbd><kbd class="key-chip">↓</kbd>Move</span>
    <span class="hint"><kbd class="key-chip">↵</kbd>Open</span>
  {/if}
  <button type="button" class="hint dismiss" onclick={escape} aria-label={escapeAction === 'Close' ? 'Close Arcade Box' : escapeAction === 'Clear' ? 'Clear the search' : 'Back to the search'}><kbd class="key-chip">Esc</kbd>{escapeAction}</button>
</footer>

<style>
  .dismiss { margin-left: auto; padding: 0; border: 0; background: none; color: inherit; font: inherit; cursor: pointer; }
  .dismiss:hover { color: var(--text); }
</style>
