<script lang="ts">
  import { useBox } from '../box.svelte';

  const box = useBox();
</script>
<footer class="hint-bar island-footer">
  {#if box.onboardingVisible}
    <span class="hint"><kbd class="key-chip">Tab</kbd>Next</span>
    <span class="hint"><kbd class="key-chip">↵</kbd>Choose</span>
  {:else if box.selectedTool}
    <span class="hint"><kbd class="key-chip">↵</kbd>Run</span>
    <span class="hint"><kbd class="key-chip">Tab</kbd>Next field</span>
  {:else}
    <span class="hint"><kbd class="key-chip">↑</kbd><kbd class="key-chip">↓</kbd>Move</span>
    <span class="hint"><kbd class="key-chip">↵</kbd>Open</span>
  {/if}
  <button type="button" class="hint dismiss" onclick={() => (box.selectedTool && !box.onboardingVisible ? box.backFromTool() : void box.dismissIsland())} aria-label={box.selectedTool && !box.onboardingVisible ? 'Back' : 'Close Arcade Box'}><kbd class="key-chip">Esc</kbd>{box.selectedTool && !box.onboardingVisible ? 'Back' : 'Close'}</button>
</footer>

<style>
  .dismiss { margin-left: auto; padding: 0; border: 0; background: none; color: inherit; font: inherit; cursor: pointer; }
  .dismiss:hover { color: var(--text); }
</style>
