<script lang="ts">
  import PluginManager from '../lib/PluginManager.svelte';
  import { useBox } from '../box.svelte';

  const box = useBox();
</script>

<header class="page-head">
  <div>
    <h1>Plugins</h1>
    <p class="sub">Add tools made by others after reviewing what they can reach.</p>
  </div>
</header>
<PluginManager onToolsChanged={box.acceptToolCatalog} />
