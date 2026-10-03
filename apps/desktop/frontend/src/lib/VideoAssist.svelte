<script lang="ts" module>
  import type { VideoPreview } from './contracts';

  // Previews are keyed by file and request shape so moving between tools or
  // reopening one does not run FFmpeg again for the same picture.
  const previewCache = new Map<string, Promise<VideoPreview>>();
</script>

<script lang="ts">
  import Icon from './Icon.svelte';
  import MediaTimeline from './MediaTimeline.svelte';
  import { bytes, clock } from './media-format';
  import { estimateVideoOutput, videoPreview } from './arcade';
  import type { MediaStreamSummary, SelectedFile, ToolSummary, VideoEstimate } from './contracts';
  import { serializeStandardUiOptions, type UiValues } from './standard-ui';

  let {
    tool,
    files,
    values,
    disabled = false,
    onValueChange,
  }: {
    tool: ToolSummary;
    files: SelectedFile[];
    values: UiValues;
    disabled?: boolean;
    onValueChange: (key: string, value: string) => void;
  } = $props();

  const THUMBNAILS = 10;
  const toolId = $derived(tool.id);
  const source = $derived(files.find((file) => file.mime === 'file/video') ?? null);
  const kind = $derived.by(() => {
    if (!source) return 'none';
    if (toolId === 'arcade.video.trim' || toolId === 'arcade.video.gif') return 'range';
    if (toolId === 'arcade.video.frames') return values.mode === 'timestamp' ? 'point' : 'range';
    if (toolId === 'arcade.video.crop') return files.length === 1 ? 'crop' : 'summary';
    if (toolId === 'arcade.video.compress') return 'estimate';
    if (toolId === 'arcade.video.extract-audio' || toolId === 'arcade.video.subtitles') return 'tracks';
    return 'summary';
  });
  const wantsThumbnails = $derived(kind === 'range' || kind === 'point');

  let preview = $state<VideoPreview | null>(null);
  let previewError = $state('');
  let loading = $state(false);

  $effect(() => {
    const token = source?.token;
    const thumbnails = wantsThumbnails ? THUMBNAILS : 0;
    const crop = kind === 'crop';
    if (!token) { preview = null; return; }
    const key = `${token}:${thumbnails}:${crop}`;
    let request = previewCache.get(key);
    if (!request) {
      request = videoPreview(token, thumbnails, crop ? -1 : undefined).catch((error) => {
        previewCache.delete(key);
        throw error;
      });
      // A negative time asks for the middle frame once the duration is known.
      previewCache.set(key, request);
    }
    let current = true;
    loading = true;
    previewError = '';
    request.then((value) => { if (current) preview = value; })
      .catch((error) => { if (current) { preview = null; previewError = typeof error === 'string' ? error : 'Could not read this video.'; } })
      .finally(() => { if (current) loading = false; });
    return () => { current = false; };
  });

  const duration = $derived(preview?.durationSeconds ?? 0);

  function number(key: string, fallback: number): number {
    const value = Number(values[key]);
    return values[key] !== '' && values[key] !== undefined && Number.isFinite(value) ? value : fallback;
  }

  function write(key: string, seconds: number): void {
    onValueChange(key, String(Math.round(seconds * 100) / 100));
  }

  // ---- Timeline -----------------------------------------------------------

  const isGif = $derived(toolId === 'arcade.video.gif');
  const rangeStart = $derived(Math.min(number('startSeconds', 0), duration || Infinity));
  const rangeEnd = $derived.by(() => {
    if (isGif) return Math.min(rangeStart + number('durationSeconds', 3), duration || Infinity);
    return Math.min(number('endSeconds', duration), duration || Infinity);
  });
  const point = $derived(Math.min(number('timestampSeconds', 0), duration || Infinity));
  function setHandle(handle: 'start' | 'end' | 'point', seconds: number): void {
    const time = Math.max(0, Math.min(duration, seconds));
    if (handle === 'point') { write('timestampSeconds', Math.min(time, Math.max(0, duration - 0.05))); return; }
    if (isGif) {
      const length = rangeEnd - rangeStart;
      if (handle === 'start') {
        const start = Math.min(time, Math.max(0, duration - 0.1));
        write('startSeconds', start);
        write('durationSeconds', Math.max(0.1, Math.min(60, length, duration - start)));
      } else {
        write('durationSeconds', Math.max(0.1, Math.min(60, time - rangeStart)));
      }
      return;
    }
    if (handle === 'start') write('startSeconds', Math.min(time, rangeEnd - 0.1));
    else write('endSeconds', Math.max(time, rangeStart + 0.1));
  }

  function moveRange(start: number): void {
    const length = rangeEnd - rangeStart;
    write('startSeconds', start);
    if (!isGif) write('endSeconds', start + length);
  }

  // ---- Crop ---------------------------------------------------------------

  type Rect = { x: number; y: number; w: number; h: number };
  const aspect = $derived.by(() => {
    const [w, h] = String(values.aspect || 'free').split(':').map(Number);
    return w > 0 && h > 0 ? w / h : null;
  });
  const frameWidth = $derived(preview?.width ?? 0);
  const frameHeight = $derived(preview?.height ?? 0);
  const explicitRect = $derived.by<Rect | null>(() => {
    const w = number('cropWidth', 0);
    const h = number('cropHeight', 0);
    return w > 0 && h > 0 ? { x: number('cropX', 0), y: number('cropY', 0), w, h } : null;
  });
  const automaticRect = $derived.by<Rect | null>(() => {
    if (!aspect || !frameWidth || !frameHeight) return null;
    let w = frameWidth;
    let h = Math.round(w / aspect);
    if (h > frameHeight) { h = frameHeight; w = Math.round(h * aspect); }
    w -= w % 2; h -= h % 2;
    return { x: Math.floor((frameWidth - w) / 2), y: Math.floor((frameHeight - h) / 2), w, h };
  });
  let draft = $state<Rect | null>(null);
  const shownRect = $derived(draft ?? explicitRect ?? automaticRect);
  let stage: HTMLDivElement | undefined = $state();
  let cropDrag: { mode: 'draw' | 'move'; ax: number; ay: number; origin: Rect | null } | null = null;

  function framePoint(event: PointerEvent): { x: number; y: number } {
    const rect = stage!.getBoundingClientRect();
    return {
      x: Math.max(0, Math.min(frameWidth, ((event.clientX - rect.left) / rect.width) * frameWidth)),
      y: Math.max(0, Math.min(frameHeight, ((event.clientY - rect.top) / rect.height) * frameHeight)),
    };
  }

  function beginCrop(event: PointerEvent): void {
    if (disabled || !stage || !frameWidth || event.button !== 0) return;
    event.preventDefault();
    const at = framePoint(event);
    const current = shownRect;
    const inside = current && at.x >= current.x && at.x <= current.x + current.w && at.y >= current.y && at.y <= current.y + current.h;
    cropDrag = inside ? { mode: 'move', ax: at.x, ay: at.y, origin: current } : { mode: 'draw', ax: at.x, ay: at.y, origin: null };
    stage.setPointerCapture(event.pointerId);
  }

  function moveCrop(event: PointerEvent): void {
    if (!cropDrag) return;
    const at = framePoint(event);
    if (cropDrag.mode === 'move' && cropDrag.origin) {
      const { w, h } = cropDrag.origin;
      draft = {
        w, h,
        x: Math.max(0, Math.min(frameWidth - w, cropDrag.origin.x + at.x - cropDrag.ax)),
        y: Math.max(0, Math.min(frameHeight - h, cropDrag.origin.y + at.y - cropDrag.ay)),
      };
      return;
    }
    let w = Math.abs(at.x - cropDrag.ax);
    let h = Math.abs(at.y - cropDrag.ay);
    if (aspect) {
      if (w / Math.max(h, 1) > aspect) w = h * aspect; else h = w / aspect;
      const maxW = at.x >= cropDrag.ax ? frameWidth - cropDrag.ax : cropDrag.ax;
      const maxH = at.y >= cropDrag.ay ? frameHeight - cropDrag.ay : cropDrag.ay;
      const fit = Math.min(1, maxW / Math.max(w, 1), maxH / Math.max(h, 1));
      w *= fit; h *= fit;
    }
    draft = {
      w, h,
      x: at.x >= cropDrag.ax ? cropDrag.ax : cropDrag.ax - w,
      y: at.y >= cropDrag.ay ? cropDrag.ay : cropDrag.ay - h,
    };
  }

  function endCrop(event: PointerEvent): void {
    if (!cropDrag) return;
    cropDrag = null;
    stage?.releasePointerCapture(event.pointerId);
    const rect = draft;
    draft = null;
    if (!rect || rect.w < 8 || rect.h < 8) return;
    const even = (value: number) => Math.max(2, Math.floor(value / 2) * 2);
    const w = Math.min(even(rect.w), frameWidth - (frameWidth % 2));
    const h = Math.min(even(rect.h), frameHeight - (frameHeight % 2));
    onValueChange('cropWidth', String(w));
    onValueChange('cropHeight', String(h));
    onValueChange('cropX', String(Math.min(Math.round(rect.x), frameWidth - w)));
    onValueChange('cropY', String(Math.min(Math.round(rect.y), frameHeight - h)));
  }

  function resetCrop(): void {
    for (const key of ['cropWidth', 'cropHeight', 'cropX', 'cropY']) onValueChange(key, '');
  }

  // ---- Compress estimate --------------------------------------------------

  let estimate = $state<VideoEstimate | null>(null);
  let estimating = $state(false);
  let estimateError = $state('');
  let estimateKey = '';
  const estimateOptions = $derived(tool.ui ? serializeStandardUiOptions(tool.ui, values) : {});
  const currentKey = $derived(`${source?.token}:${JSON.stringify(estimateOptions)}`);

  $effect(() => {
    // Any option change makes an earlier estimate stale.
    if (currentKey !== estimateKey) { estimate = null; estimateError = ''; }
    if (kind === 'estimate' && values.mode === 'targetSize' && source && currentKey !== estimateKey && !estimating) void runEstimate();
  });

  async function runEstimate(): Promise<void> {
    if (!source) return;
    const key = currentKey;
    estimateKey = key;
    estimating = true;
    estimateError = '';
    try {
      const value = await estimateVideoOutput(source.token, estimateOptions);
      if (key === currentKey) estimate = value;
    } catch (error) {
      if (key === currentKey) estimateError = typeof error === 'string' ? error : 'Could not estimate the size.';
    } finally {
      estimating = false;
    }
  }

  const saving = $derived(estimate?.sourceBytes ? 1 - estimate.estimatedBytes / estimate.sourceBytes : null);

  // ---- Tracks ---------------------------------------------------------------

  const tracks = $derived.by<MediaStreamSummary[]>(() => {
    if (!preview) return [];
    if (toolId === 'arcade.video.extract-audio') return preview.audio;
    if (values.operation === 'extract' || values.operation === 'burn') return preview.subtitles;
    return [];
  });
  const trackKey = $derived(toolId === 'arcade.video.extract-audio' ? 'track' : 'streamIndex');
  const selectedTrack = $derived(number(trackKey, 0));
  const showTracks = $derived(kind === 'tracks' && (toolId === 'arcade.video.subtitles'
    ? (values.operation === 'extract' && values.allStreams !== 'true') || (values.operation === 'burn' && files.length === 1)
    : preview !== null && preview.audio.length > 1));
</script>

{#if source}
  <section class="video-assist" aria-label="Video preview" aria-busy={loading}>
    {#if previewError}
      <p class="assist-error" role="alert">{previewError}</p>
    {:else if preview}
      <div class="assist-meta">
        <span>{preview.width && preview.height ? `${preview.width}×${preview.height}` : 'Audio only'}</span>
        {#if preview.frameRate}<span>{preview.frameRate.toFixed(preview.frameRate % 1 ? 2 : 0)} fps</span>{/if}
        {#if preview.videoCodec}<span>{preview.videoCodec}</span>{/if}
        {#if duration}<span>{clock(duration)}</span>{/if}
        <span>{bytes(preview.sourceBytes)}</span>
        {#if files.length > 1 && kind !== 'tracks'}<span class="assist-note">Showing the first of {files.length} files</span>{/if}
      </div>

      {#if (kind === 'range' || kind === 'point') && duration > 0}
        <MediaTimeline
          {duration}
          mode={kind === 'range' ? 'range' : 'point'}
          start={rangeStart}
          end={rangeEnd}
          {point}
          step={preview.frameRate ? 1 / preview.frameRate : 0.1}
          {disabled}
          onSeek={setHandle}
          onMove={moveRange}
        >
          {#snippet background()}
            <div class="filmstrip">
              {#each preview!.thumbnails as frame (frame.timeSeconds)}<img src={frame.dataUrl} alt="" draggable="false" />{/each}
            </div>
          {/snippet}
        </MediaTimeline>
        <div class="timeline-readout">
          {#if kind === 'range'}
            <span>{clock(rangeStart)}</span><strong>{(rangeEnd - rangeStart).toFixed(2)} s selected</strong><span>{clock(rangeEnd)}</span>
          {:else}
            <span>Frame at {clock(point)}</span><span class="assist-note">Arrow keys step one frame · Shift steps 1 s</span>
          {/if}
        </div>
      {/if}

      {#if kind === 'crop' && preview.frame && frameWidth && frameHeight}
        <div
          class="crop-stage"
          bind:this={stage}
          style:aspect-ratio={`${frameWidth} / ${frameHeight}`}
          style:width={`min(100%, ${Math.round(300 * frameWidth / frameHeight)}px)`}
          role="group"
          aria-label="Crop area. Drag to draw a region or move it."
          onpointerdown={beginCrop}
          onpointermove={moveCrop}
          onpointerup={endCrop}
          onpointercancel={endCrop}
        >
          <img src={preview.frame.dataUrl} alt="" draggable="false" />
          {#if shownRect}
            <div
              class="crop-rect"
              class:automatic={!draft && !explicitRect}
              style:left={`${(shownRect.x / frameWidth) * 100}%`}
              style:top={`${(shownRect.y / frameHeight) * 100}%`}
              style:width={`${(shownRect.w / frameWidth) * 100}%`}
              style:height={`${(shownRect.h / frameHeight) * 100}%`}
            ></div>
          {/if}
        </div>
        <div class="timeline-readout">
          {#if shownRect}
            <span>{Math.round(shownRect.w)}×{Math.round(shownRect.h)} at {Math.round(shownRect.x)}, {Math.round(shownRect.y)}{!draft && !explicitRect ? ' · centered' : ''}</span>
            {#if explicitRect}<button type="button" class="quiet-button" onclick={resetCrop} {disabled}>Reset area</button>{/if}
          {:else}
            <span class="assist-note">Drag on the frame to choose an area, or pick an aspect ratio.</span>
          {/if}
        </div>
        {#if values.rotation !== '0' || (values.flip && values.flip !== 'none')}<p class="assist-note">Rotation and flip are applied after cropping.</p>{/if}
      {/if}

      {#if kind === 'estimate'}
        <div class="estimate-row">
          <div class="estimate-copy">
            {#if estimate}
              <strong>≈ {bytes(estimate.estimatedBytes)}</strong>
              <span>{saving !== null ? (saving > 0 ? `${Math.round(saving * 100)}% smaller than ${bytes(estimate.sourceBytes)}` : `not smaller than ${bytes(estimate.sourceBytes)}`) : ''}</span>
            {:else if estimateError}
              <span class="assist-error" role="alert">{estimateError}</span>
            {:else}
              <span>{values.mode === 'targetSize' ? 'Checking the target…' : 'See the result size before you run.'}</span>
            {/if}
          </div>
          {#if values.mode !== 'targetSize'}
            <button type="button" class="quiet-button" onclick={() => void runEstimate()} disabled={disabled || estimating}>
              {#if estimating}<span class="spinner"></span><span>Estimating…</span>{:else}<Icon name="spark" size={14} /><span>{estimate ? 'Estimate again' : 'Estimate size'}</span>{/if}
            </button>
          {/if}
        </div>
        {#if estimate?.method === 'sample'}<p class="assist-note">Measured by encoding {Math.round(estimate.sampledSeconds)} s of samples with these settings.</p>{/if}
        {#each estimate?.warnings ?? [] as warning}<p class="assist-note warning">{warning}</p>{/each}
      {/if}

      {#if showTracks}
        {#if tracks.length}
          <div class="track-list" role="radiogroup" aria-label={toolId === 'arcade.video.extract-audio' ? 'Audio track' : 'Subtitle track'}>
            {#each tracks as item (item.index)}
              <button type="button" role="radio" aria-checked={selectedTrack === item.index} class:active={selectedTrack === item.index} {disabled}
                onclick={() => onValueChange(trackKey, String(item.index))}>
                <span class="track-index">{item.index}</span>
                <span class="track-copy"><strong>{item.language ?? 'Unknown language'}{item.title ? ` · ${item.title}` : ''}</strong><small>{item.detail}</small></span>
                {#if item.bitmap}<span class="track-chip">Image-based</span>{/if}
              </button>
            {/each}
          </div>
        {:else if toolId === 'arcade.video.subtitles'}
          <p class="assist-note">This video has no embedded subtitle tracks.</p>
        {/if}
      {/if}
    {:else}
      <div class="assist-loading"><span class="spinner"></span><span>Reading video…</span></div>
    {/if}
  </section>
{/if}

<style>
  .video-assist { display: grid; gap: 10px; margin: 12px 14px 0; }
  .assist-meta { display: flex; flex-wrap: wrap; gap: 4px 12px; color: var(--ink-3); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .assist-note { margin: 0; color: var(--ink-3); font-size: 11.5px; line-height: 1.45; }
  .assist-note.warning { color: var(--amber); }
  .assist-error { margin: 0; color: var(--red); font-size: 12px; line-height: 1.45; }
  .assist-loading { display: flex; align-items: center; gap: 8px; min-height: 56px; color: var(--ink-3); font-size: 12px; }

  .filmstrip { display: flex; width: 100%; height: 100%; }
  .filmstrip img { min-width: 0; height: 100%; flex: 1 1 0; object-fit: cover; pointer-events: none; }
  .timeline-readout { display: flex; align-items: center; justify-content: space-between; gap: 10px; min-height: 22px; color: var(--ink-3); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .timeline-readout strong { color: var(--ink); font-weight: 550; }

  .crop-stage { position: relative; margin: 0 auto; overflow: hidden; border-radius: 9px; background: #000; cursor: crosshair; touch-action: none; user-select: none; }
  .crop-stage img { display: block; width: 100%; height: 100%; object-fit: fill; pointer-events: none; }
  .crop-rect { position: absolute; border: 1.5px solid #fff; border-radius: 2px; box-shadow: 0 0 0 9999px rgba(0, 0, 0, .5); cursor: move; pointer-events: none; }
  .crop-rect.automatic { border-style: dashed; }

  .estimate-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 40px; padding: 8px 8px 8px 12px; border-radius: 10px; background: var(--surface-1); }
  .estimate-copy { display: grid; gap: 1px; min-width: 0; font-size: 12px; color: var(--ink-3); }
  .estimate-copy strong { color: var(--ink); font-size: 14px; font-weight: 600; font-variant-numeric: tabular-nums; }

  .track-list { display: grid; gap: 2px; }
  .track-list button { display: flex; align-items: center; gap: 10px; min-height: 40px; padding: 6px 10px; border: 0; border-radius: 9px; color: inherit; background: transparent; text-align: left; cursor: pointer; transition: background-color 80ms ease; }
  .track-list button:hover:not(:disabled) { background: var(--surface-1); }
  .track-list button.active { background: var(--surface-3); }
  .track-index { display: grid; place-items: center; width: 22px; height: 22px; flex: 0 0 auto; border-radius: 6px; color: var(--ink-2); background: var(--surface-2); font-size: 11px; font-variant-numeric: tabular-nums; }
  .track-copy { display: grid; min-width: 0; flex: 1; gap: 1px; }
  .track-copy strong { overflow: hidden; color: var(--ink); font-size: 12.5px; font-weight: 550; text-overflow: ellipsis; white-space: nowrap; text-transform: capitalize; }
  .track-copy small { color: var(--ink-3); font-size: 11.5px; }
  .track-chip { padding: 2px 6px; border-radius: 6px; color: var(--amber); background: var(--amber-wash); font-size: 10.5px; font-weight: 600; }
</style>
