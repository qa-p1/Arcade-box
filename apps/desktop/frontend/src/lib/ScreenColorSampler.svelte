<script lang="ts">
  import { copyText } from './arcade';
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { sampleScreenImagePixel, screenImagePreview } from './arcade';
  import type { SampledScreenPixel, ScreenImagePreview } from './contracts';

  let { token }: { token: string } = $props();
  let preview = $state<ScreenImagePreview | null>(null);
  let pixel = $state<SampledScreenPixel | null>(null);
  let busy = $state(false);
  let error = $state('');
  let copyState = $state('');
  let copyTimer: number | undefined;
  let requestSerial = 0;
  let imageElement: HTMLImageElement | undefined = $state();
  let marker = $state({ x: 0.5, y: 0.5 });

  onMount(() => {
    void preparePreview();
    return () => {
      if (copyTimer !== undefined) window.clearTimeout(copyTimer);
    };
  });

  async function preparePreview(): Promise<void> {
    busy = true;
    error = '';
    try {
      preview = await screenImagePreview(token, 512);
      marker = { x: 0.5, y: 0.5 };
      await sampleAt(Math.floor(preview.sourceWidth / 2), Math.floor(preview.sourceHeight / 2));
    } catch (cause) {
      error = messageOf(cause);
    } finally {
      busy = false;
    }
  }

  function updateMarker(event: PointerEvent): { x: number; y: number } | null {
    if (!imageElement || !preview) return null;
    const rect = imageElement.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    const x = Math.min(0.999999, Math.max(0, (event.clientX - rect.left) / rect.width));
    const y = Math.min(0.999999, Math.max(0, (event.clientY - rect.top) / rect.height));
    marker = { x, y };
    return {
      x: Math.floor(x * preview.sourceWidth),
      y: Math.floor(y * preview.sourceHeight),
    };
  }

  function previewPointerMove(event: PointerEvent): void {
    updateMarker(event);
  }

  async function previewPointerDown(event: PointerEvent): Promise<void> {
    const coordinates = updateMarker(event);
    if (coordinates) await sampleAt(coordinates.x, coordinates.y);
  }

  async function previewKeyDown(event: KeyboardEvent): Promise<void> {
    if (!preview) return;
    const currentX = pixel?.x ?? Math.floor(preview.sourceWidth / 2);
    const currentY = pixel?.y ?? Math.floor(preview.sourceHeight / 2);
    let x = currentX;
    let y = currentY;
    if (event.key === 'ArrowLeft') x = Math.max(0, x - 1);
    else if (event.key === 'ArrowRight') x = Math.min(preview.sourceWidth - 1, x + 1);
    else if (event.key === 'ArrowUp') y = Math.max(0, y - 1);
    else if (event.key === 'ArrowDown') y = Math.min(preview.sourceHeight - 1, y + 1);
    else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      await sampleAt(currentX, currentY);
      return;
    }
    else if (event.key === 'Home') {
      x = Math.floor(preview.sourceWidth / 2);
      y = Math.floor(preview.sourceHeight / 2);
    } else return;
    event.preventDefault();
    marker = {
      x: (x + 0.5) / preview.sourceWidth,
      y: (y + 0.5) / preview.sourceHeight,
    };
    await sampleAt(x, y);
  }

  async function sampleAt(x: number, y: number): Promise<void> {
    const request = ++requestSerial;
    busy = true;
    error = '';
    try {
      const sampled = await sampleScreenImagePixel(token, x, y);
      if (request !== requestSerial) return;
      pixel = sampled;
      if (preview) marker = {
        x: (sampled.x + 0.5) / preview.sourceWidth,
        y: (sampled.y + 0.5) / preview.sourceHeight,
      };
    } catch (cause) {
      if (request === requestSerial) error = messageOf(cause);
    } finally {
      if (request === requestSerial) busy = false;
    }
  }

  async function copy(value: string, kind: string): Promise<void> {
    try {
      await copyText(value);
      copyState = kind;
      if (copyTimer !== undefined) window.clearTimeout(copyTimer);
      copyTimer = window.setTimeout(() => (copyState = ''), 1600);
    } catch {
      error = 'Clipboard access was not available. Select and copy the value instead.';
    }
  }

  function messageOf(cause: unknown): string {
    return typeof cause === 'string' ? cause : cause instanceof Error ? cause.message : 'Could not sample this screen image.';
  }

  function hslText(): string {
    return pixel ? `hsl(${pixel.hsl[0].toFixed(1)} ${pixel.hsl[1].toFixed(1)}% ${pixel.hsl[2].toFixed(1)}%)` : '';
  }
</script>

<section class="screen-color-sampler" aria-label="Screen pixel color picker">
  {#if preview}
    <div class="screen-color-workspace">
      <div class="screen-color-canvas-wrap">
        <button
          type="button"
          class="screen-color-canvas"
          class:busy
          aria-label="Captured screen image. Click a pixel to sample it. Use the arrow keys to move one source pixel at a time, or Home to return to the center."
          onpointermove={previewPointerMove}
          onpointerdown={(event) => void previewPointerDown(event)}
          onkeydown={(event) => void previewKeyDown(event)}
        >
          <img bind:this={imageElement} src={preview.dataUrl} alt="Captured screen region to sample" draggable="false" />
          <span class="screen-color-crosshair" aria-hidden="true" style={`left:${marker.x * 100}%;top:${marker.y * 100}%`}></span>
        </button>
        <div class="screen-color-image-meta"><span>{preview.sourceWidth} × {preview.sourceHeight} pixels</span><span>Click or focus and use arrow keys</span></div>
      </div>
      <aside class="screen-color-readout" aria-live="polite" aria-atomic="true">
        {#if pixel}
          <div class="screen-color-magnifier"><img src={pixel.magnifierDataUrl} alt="Magnified 11 by 11 pixel neighborhood" /><span class="screen-color-center" aria-hidden="true"></span></div>
          <div class="screen-color-swatch-row"><span class="screen-color-swatch" style={`background-color:${pixel.hex}`} aria-label={`Sampled color ${pixel.hex}`}></span><span><strong>{pixel.hex}</strong><small>Pixel {pixel.x}, {pixel.y}{pixel.rgba[3] < 255 ? ` · alpha ${pixel.rgba[3]}` : ''}</small></span></div>
          <div class="screen-color-values">
            <div><span>RGB</span><code>{pixel.rgb.join(', ')}</code><button type="button" class="icon-button" aria-label="Copy RGB color" onclick={() => void copy(`rgb(${pixel?.rgb.join(', ') ?? ''})`, 'rgb')}><Icon name={copyState === 'rgb' ? 'check' : 'copy'} size={13} /></button></div>
            <div><span>HSL</span><code>{hslText()}</code><button type="button" class="icon-button" aria-label="Copy HSL color" onclick={() => void copy(hslText(), 'hsl')}><Icon name={copyState === 'hsl' ? 'check' : 'copy'} size={13} /></button></div>
          </div>
          <button type="button" class="copy-button screen-color-copy" onclick={() => void copy(pixel?.hex || '', 'hex')}><Icon name={copyState === 'hex' ? 'check' : 'copy'} size={14} />{copyState === 'hex' ? 'HEX copied' : 'Copy HEX'}</button>
        {:else}
          <div class="screen-color-placeholder"><span class="spinner"></span><strong>{busy ? 'Reading pixel color…' : 'Choose a pixel'}</strong></div>
        {/if}
      </aside>
    </div>
    {#if busy}<p class="screen-color-status" role="status">Reading the exact color from the original captured image.</p>{/if}
  {:else}
    <div class="screen-color-placeholder"><span class="spinner"></span><strong>Preparing screen image…</strong></div>
  {/if}
  {#if error}<div class="field-error" role="alert">{error}</div>{/if}
</section>
