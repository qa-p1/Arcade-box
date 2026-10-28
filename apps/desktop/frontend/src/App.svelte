<script lang="ts">
  import { createBox, provideBox } from './box.svelte';
  import Island from './island/Island.svelte';
  import Dashboard from './dashboard/Dashboard.svelte';

  const box = provideBox(createBox());
</script>

<svelte:window onkeydown={box.handleWindowKeydown} />

<main class:surface-hidden={!box.surfaceVisible} class:dismissing={box.dismissing} class:dashboard-mode={box.surface === 'dashboard'} class:theme-light={box.themeMode === 'light'} class:theme-dark={box.themeMode === 'dark'} class:theme-system={box.themeMode === 'system'} class="app-frame">
  <div class="sr-only" role="status" aria-live="polite" aria-atomic="true">{box.searchAnnouncement}</div>
  <div class="sr-only" role="status" aria-live="polite" aria-atomic="true">{box.resultAnnouncement}</div>
  <span class="sr-only" id="search-keyboard-help">Use the up and down arrow keys to move through available actions, Enter to open or run one, and Escape to clear the search or close Arcade Box.</span>
  {#if box.surface === 'island'}
    <Island />
  {:else}
    <Dashboard />
  {/if}
</main>
