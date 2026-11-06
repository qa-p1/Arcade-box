<script lang="ts">
  import Sidebar from './Sidebar.svelte';
  import ToolsPage from './ToolsPage.svelte';
  import JobsPage from './JobsPage.svelte';
  import PipelinesPage from './PipelinesPage.svelte';
  import PluginsPage from './PluginsPage.svelte';
  import EnginesPage from './EnginesPage.svelte';
  import SettingsPage from './SettingsPage.svelte';
  import { useBox } from '../box.svelte';

  const box = useBox();
  let content: HTMLElement;

  $effect(() => {
    // Each page/category starts at its heading, independently of the sidebar.
    if (box.catalogMode && box.categoryFilter) content?.scrollTo(0, 0);
  });
</script>

<div class="app-shell dashboard-shell">
  <Sidebar />
  <!-- svelte-ignore a11y_no_noninteractive_tabindex (the scrollable landmark needs keyboard focus for PageDown/Home/End) -->
  <main class="shell-main" bind:this={content} tabindex="0" aria-label="Dashboard content">
    <div class="page-main">
      {#if box.catalogMode === 'jobs'}<JobsPage />
      {:else if box.catalogMode === 'pipelines'}<PipelinesPage />
      {:else if box.catalogMode === 'plugins'}<PluginsPage />
      {:else if box.catalogMode === 'engines'}<EnginesPage />
      {:else if box.catalogMode === 'settings'}<SettingsPage />
      {:else}<ToolsPage />{/if}
    </div>
  </main>
</div>
