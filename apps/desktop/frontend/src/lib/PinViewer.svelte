<script lang="ts">
  // A frozen screen region kept above other windows. Hover shows the kit
  // toolbar: opacity, copy and close.
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Icon from './Icon.svelte';

  interface PinImage {
    dataUrl: string;
    width: number;
    height: number;
  }

  const OPACITY = [1, 0.7, 0.4];
  let image = $state<PinImage | null>(null);
  let error = $state('');
  let opacity = $state(0);
  let status = $state('');

  onMount(() => {
    const injected = (window as Window & { __ARCADE_PIN_IMAGE?: PinImage }).__ARCADE_PIN_IMAGE;
    if (!injected || !injected.dataUrl.startsWith('data:image/png;base64,') || injected.dataUrl.length > 1_500_000) {
      error = "This pinned image isn't available.";
      return;
    }
    image = injected;
  });

  function say(text: string): void {
    status = text;
    window.setTimeout(() => { if (status === text) status = ''; }, 1600);
  }

  async function copy(): Promise<void> {
    if (!image) return;
    try {
      const blob = await (await fetch(image.dataUrl)).blob();
      await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
      say('Copied');
    } catch {
      say("Couldn't copy the image");
    }
  }
</script>

<svelte:window onkeydown={(event) => { if (event.key === 'Escape') void getCurrentWindow().close(); }} />

<main class="pin-window" aria-label="Pinned reference">
  {#if image}
    <div class="pin-image-stage"><img src={image.dataUrl} alt="Pinned screen region" draggable="false" style:opacity={OPACITY[opacity]} /></div>
    <span class="pin-size">{image.width} × {image.height}</span>
    <div class="pin-toolbar" role="toolbar" aria-label="Pinned reference">
      <button type="button" class="btn ghost icon-only sm" aria-label="Opacity {Math.round(OPACITY[opacity] * 100)}%" title="Opacity" onclick={() => { opacity = (opacity + 1) % OPACITY.length; }}><Icon name="eye" size={14} /></button>
      <button type="button" class="btn ghost icon-only sm" aria-label="Copy" title="Copy" onclick={() => void copy()}><Icon name="copy" size={14} /></button>
      <button type="button" class="btn ghost icon-only sm" aria-label="Close" title="Close (Esc)" onclick={() => void getCurrentWindow().close()}><Icon name="close" size={14} /></button>
    </div>
    {#if status}<span class="pin-status" role="status">{status}</span>{/if}
  {:else if error}
    <div class="pin-message" role="alert">{error}</div>
  {:else}
    <div class="pin-message" role="status">Preparing the image…</div>
  {/if}
</main>
