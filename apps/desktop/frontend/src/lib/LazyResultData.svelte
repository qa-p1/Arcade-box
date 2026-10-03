<script lang="ts">
  import ResultData from './ResultData.svelte';
  let { value, depth }: { value: unknown; depth: number } = $props();
  let container: HTMLDivElement;
  let open = $state(false);
  $effect(() => {
    const details = container?.closest('details');
    if (!details) return;
    const update = () => { open = details.open; };
    update();
    details.addEventListener('toggle', update);
    return () => details.removeEventListener('toggle', update);
  });
</script>
<div bind:this={container}>{#if open}<ResultData {value} {depth} />{/if}</div>
