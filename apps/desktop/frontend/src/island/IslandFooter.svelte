<script lang="ts">
  import { useBox } from '../box.svelte';

  const box = useBox();
</script>
<footer class="island-footer">
  <div class="footer-left">{#if box.selectedTool}<span class="key-hint"><kbd>Alt</kbd><kbd>←</kbd><span>back</span></span>{:else}<span class="key-hint"><kbd>↑</kbd><kbd>↓</kbd><span>navigate</span></span><span class="key-hint"><kbd>↵</kbd><span>open</span></span>{/if}</div>
  <div class="footer-right"><span class="privacy-footer"><span></span>Private by default</span><button class="dismiss-button" onclick={() => void box.dismissIsland()} aria-label="Close Arcade Box"><kbd>Esc</kbd><span>close</span></button></div>
</footer>
