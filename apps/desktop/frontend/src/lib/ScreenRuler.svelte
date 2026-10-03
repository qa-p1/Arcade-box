<script lang="ts">
  import { copyText } from './arcade';
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { measureScreenArea, screenImagePreview } from './arcade';
  import type { ScreenImagePreview, ScreenMeasurement } from './contracts';

  let { token }: { token: string } = $props();
  let preview = $state<ScreenImagePreview | null>(null);
  let anchor = $state<[number, number] | null>(null);
  let cursor = $state<[number, number] | null>(null);
  let measurement = $state<ScreenMeasurement | null>(null);
  let busy = $state(false);
  let error = $state('');
  let announcement = $state('');
  let copyState = $state(false);
  let copyTimer: number | undefined;
  let imageElement: HTMLImageElement | undefined = $state();
  let selectionVersion = 0;

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
      cursor = [Math.floor(preview.sourceWidth / 2), Math.floor(preview.sourceHeight / 2)];
    } catch (cause) {
      error = messageOf(cause);
    } finally {
      busy = false;
    }
  }

  function pointFromPointer(event: PointerEvent): [number, number] | null {
    if (!imageElement || !preview) return null;
    const bounds = imageElement.getBoundingClientRect();
    if (bounds.width <= 0 || bounds.height <= 0) return null;
    const x = Math.min(0.999999, Math.max(0, (event.clientX - bounds.left) / bounds.width));
    const y = Math.min(0.999999, Math.max(0, (event.clientY - bounds.top) / bounds.height));
    return [Math.floor(x * preview.sourceWidth), Math.floor(y * preview.sourceHeight)];
  }

  function pointerMove(event: PointerEvent): void {
    const point = pointFromPointer(event);
    if (point) cursor = point;
  }

  async function pointerDown(event: PointerEvent): Promise<void> {
    const point = pointFromPointer(event);
    if (point) {
      cursor = point;
      await activatePoint(point);
    }
  }

  async function keyDown(event: KeyboardEvent): Promise<void> {
    if (!preview) return;
    let [x, y] = cursor || [Math.floor(preview.sourceWidth / 2), Math.floor(preview.sourceHeight / 2)];
    if (event.key === 'ArrowLeft') x = Math.max(0, x - 1);
    else if (event.key === 'ArrowRight') x = Math.min(preview.sourceWidth - 1, x + 1);
    else if (event.key === 'ArrowUp') y = Math.max(0, y - 1);
    else if (event.key === 'ArrowDown') y = Math.min(preview.sourceHeight - 1, y + 1);
    else if (event.key === 'Home') {
      x = Math.floor(preview.sourceWidth / 2);
      y = Math.floor(preview.sourceHeight / 2);
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      await activatePoint([x, y]);
      return;
    } else return;
    event.preventDefault();
    cursor = [x, y];
  }

  async function activatePoint(point: [number, number]): Promise<void> {
    cursor = point;
    error = '';
    if (!anchor || measurement) {
      anchor = point;
      measurement = null;
      announcement = `Start point set at ${point[0]}, ${point[1]}. Choose an end point.`;
      selectionVersion++;
      return;
    }
    const version = ++selectionVersion;
    busy = true;
    announcement = 'Measuring selected screen pixels.';
    try {
      const result = await measureScreenArea(token, anchor, point);
      if (version !== selectionVersion) return;
      const output = result.outputs.find((item) => item.mime === 'structured/screen-measurement');
      if (result.status !== 'success' || !output) throw new Error(result.message || 'Screen measurement was not returned.');
      measurement = JSON.parse(output.value) as ScreenMeasurement;
      announcement = `Measured ${measurement.widthPixels} by ${measurement.heightPixels} pixels; diagonal ${measurement.diagonalDistance.toFixed(1)} pixels.`;
    } catch (cause) {
      if (version === selectionVersion) error = messageOf(cause);
    } finally {
      if (version === selectionVersion) busy = false;
    }
  }

  async function copyMeasurement(): Promise<void> {
    if (!measurement) return;
    const text = `Selection: ${measurement.widthPixels} × ${measurement.heightPixels} px\nHorizontal: ${measurement.horizontalDistance} px\nVertical: ${measurement.verticalDistance} px\nDiagonal: ${measurement.diagonalDistance.toFixed(2)} px`;
    try {
      await copyText(text);
      copyState = true;
      if (copyTimer !== undefined) window.clearTimeout(copyTimer);
      copyTimer = window.setTimeout(() => (copyState = false), 1500);
    } catch {
      error = 'Clipboard access was not available. Select the measurements to copy them.';
    }
  }

  function messageOf(cause: unknown): string {
    return typeof cause === 'string' ? cause : cause instanceof Error ? cause.message : 'Could not measure this screen image.';
  }

  function pointStyle(point: [number, number] | null): string {
    if (!point || !preview) return 'display:none';
    return `left:${((point[0] + 0.5) / preview.sourceWidth) * 100}%;top:${((point[1] + 0.5) / preview.sourceHeight) * 100}%`;
  }
</script>

<section class="screen-ruler" aria-label="Screen pixel measurement">
  {#if preview}
    <div class="screen-ruler-canvas-wrap">
      <button
        type="button"
        class="screen-ruler-canvas"
        class:busy
        style={`width:min(100%, ${Math.max(120, Math.round(300 * preview.width / preview.height))}px);aspect-ratio:${preview.width}/${preview.height}`}
        aria-label="Captured screen image. Click once to set a start point and again to measure. Use arrow keys to position the point; Enter selects it."
        onpointermove={pointerMove}
        onpointerdown={(event) => void pointerDown(event)}
        onkeydown={(event) => void keyDown(event)}
      >
        <img bind:this={imageElement} src={preview.dataUrl} alt="Captured screen region for pixel measurement" draggable="false" />
        {#if anchor && cursor && !measurement}
          <svg class="screen-ruler-line" viewBox={`0 0 ${preview.width} ${preview.height}`} preserveAspectRatio="none" aria-hidden="true">
            <line x1={anchor[0] * preview.width / preview.sourceWidth} y1={anchor[1] * preview.height / preview.sourceHeight} x2={cursor[0] * preview.width / preview.sourceWidth} y2={cursor[1] * preview.height / preview.sourceHeight} />
          </svg>
          <span class="screen-ruler-anchor" style={pointStyle(anchor)} aria-hidden="true"></span>
        {/if}
        <span class="screen-ruler-cursor" style={pointStyle(cursor)} aria-hidden="true"></span>
      </button>
      <div class="screen-color-image-meta"><span>{preview.sourceWidth} × {preview.sourceHeight} captured pixels</span><span>Click, or use arrow keys + Enter</span></div>
    </div>
    {#if measurement}
      <div class="screen-ruler-readout" aria-live="polite" aria-atomic="true">
        <div><span>Selection width</span><strong>{measurement.widthPixels} px</strong></div>
        <div><span>Selection height</span><strong>{measurement.heightPixels} px</strong></div>
        <div><span>Horizontal distance</span><strong>{measurement.horizontalDistance} px</strong></div>
        <div><span>Vertical distance</span><strong>{measurement.verticalDistance} px</strong></div>
        <div><span>Diagonal distance</span><strong>{measurement.diagonalDistance.toFixed(2)} px</strong></div>
        <div><span>Start / end</span><strong>{measurement.start.join(', ')} → {measurement.end.join(', ')}</strong></div>
        <button type="button" class="copy-button" onclick={() => void copyMeasurement()}><Icon name={copyState ? 'check' : 'copy'} size={13} />{copyState ? 'Measurements copied' : 'Copy measurements'}</button>
      </div>
    {:else}
      <p class="screen-ruler-instructions" role="status">{announcement || (anchor ? `Start point ${anchor[0]}, ${anchor[1]} selected. Choose an end point.` : 'Click a start point, then an end point. Arrow keys position the cursor precisely.')}</p>
    {/if}
  {:else}
    <div class="screen-color-placeholder"><span class="spinner"></span><strong>Preparing screen image…</strong></div>
  {/if}
  {#if error}<div class="field-error" role="alert">{error}</div>{/if}
</section>
