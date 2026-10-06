<script lang="ts">
  import { onMount } from 'svelte';
  import { shortcutOwner, watchLinkChanged } from './arcade';
  let { accelerator }: { accelerator: string } = $props();
  let owner = $state<string | null>(null);
  let revision = $state(0);
  $effect(() => {
    const value = accelerator; revision;
    let current = true;
    void shortcutOwner(value).then((name) => { if (current) owner = name; }).catch(() => { if (current) owner = null; });
    return () => { current = false; };
  });
  onMount(() => {
    let disposed = false;
    let stop = () => {};
    void watchLinkChanged(() => revision++).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    return () => { disposed = true; stop(); };
  });
</script>

{#if owner}<p class="shortcut-clash" role="status">Used by {owner}</p>{/if}

<style>
  .shortcut-clash { color: var(--muted); font-size: 12px; margin: 8px 0; }
</style>
