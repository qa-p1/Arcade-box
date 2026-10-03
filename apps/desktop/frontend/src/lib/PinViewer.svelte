<script lang="ts">
  import { onMount } from 'svelte';

  interface PinImage {
    dataUrl: string;
    width: number;
    height: number;
  }

  let image = $state<PinImage | null>(null);
  let error = $state('');

  onMount(() => {
    const injected = (window as Window & { __ARCADE_PIN_IMAGE?: PinImage }).__ARCADE_PIN_IMAGE;
    if (!injected || !injected.dataUrl.startsWith('data:image/png;base64,') || injected.dataUrl.length > 1_500_000) {
      error = 'This pinned image preview is unavailable.';
      return;
    }
    image = injected;
  });
</script>

<main class="pin-window" aria-label="Pinned Arcade Box image">
  <header class="pin-window-header"><span class="pin-window-indicator"></span><strong>Pinned reference</strong>{#if image}<span>{image.width} × {image.height}</span>{/if}</header>
  {#if image}
    <div class="pin-image-stage"><img src={image.dataUrl} alt="Frozen screen region" draggable="false" /></div>
  {:else if error}
    <div class="pin-window-error" role="alert">{error}</div>
  {:else}
    <div class="pin-window-loading" role="status">Preparing image…</div>
  {/if}
</main>
