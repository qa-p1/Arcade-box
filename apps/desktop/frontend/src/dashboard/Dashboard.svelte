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
</script>

<div class="app-shell dashboard-shell">
  <Sidebar />
  <main class="shell-main">
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
