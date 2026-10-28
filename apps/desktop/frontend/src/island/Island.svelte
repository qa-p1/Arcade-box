<script lang="ts">
  import IslandTool from './IslandTool.svelte';
  import IslandSearch from './IslandSearch.svelte';
  import IslandFooter from './IslandFooter.svelte';
  import { useBox } from '../box.svelte';

  const box = useBox();
</script>
<section bind:this={box.islandShell} class:expanded={!box.isCompact} class:compact={box.isCompact} class:tool-open={Boolean(box.selectedTool)} class:dismissing={box.dismissing} style:height={box.islandHeight ? `${box.islandHeight}px` : undefined} class="island-shell" aria-label="Arcade Box action surface">
  <div class="island-content" bind:this={box.islandContent}>
  {#if box.selectedTool}
    <IslandTool />
  {:else}
    <IslandSearch />
  {/if}

  <IslandFooter />
  </div>
</section>
