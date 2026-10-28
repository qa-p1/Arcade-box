<script lang="ts">
  import PipelinesView from '../lib/PipelinesView.svelte';
  import { useBox } from '../box.svelte';

  const box = useBox();
</script>

<header class="page-head">
  <div>
    <h1>Pipelines</h1>
    <p class="sub">Run several tools in a row as one step.</p>
  </div>
</header>
<PipelinesView tools={box.tools} onToolsChanged={box.acceptToolCatalog} />
