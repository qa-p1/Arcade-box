<script lang="ts">
  import { onMount } from 'svelte';

  let { rgbaBase64, width, height }: { rgbaBase64: string; width: number; height: number } = $props();
  let canvas: HTMLCanvasElement | undefined = $state();
  let error = $state(false);

  onMount(() => {
    if (!canvas || width < 1 || height < 1 || width > 4096 || height > 4096) {
      error = true;
      return;
    }
    try {
      const binary = atob(rgbaBase64);
      const expectedBytes = width * height * 4;
      if (binary.length !== expectedBytes || expectedBytes > 8 * 1024 * 1024) throw new Error('Invalid image size');
      const pixels = new Uint8ClampedArray(expectedBytes);
      for (let index = 0; index < expectedBytes; index += 1) pixels[index] = binary.charCodeAt(index);
      const context = canvas.getContext('2d');
      if (!context) throw new Error('Canvas is unavailable');
      context.putImageData(new ImageData(pixels, width, height), 0, 0);
    } catch {
      error = true;
    }
  });
</script>

{#if error}
  <span class="clipboard-image-fallback" role="status">Image preview unavailable</span>
{:else}
  <canvas bind:this={canvas} {width} {height} aria-label="Clipboard image preview"></canvas>
{/if}

<style>
  canvas { display:block; width:auto; max-width:100%; max-height:132px; object-fit:contain; border:1px solid var(--border-subtle); border-radius:6px; background:#fff; }
  .clipboard-image-fallback { color:var(--text-muted); font-size:11px; }
</style>
