<script lang="ts" module>
  import type { AudioPreview, LoudnessReport } from './contracts';

  // Keyed by file and request shape, so reopening a tool or switching between
  // audio tools does not decode the same file again.
  const previewCache = new Map<string, Promise<AudioPreview>>();
  const loudnessCache = new Map<string, Promise<LoudnessReport>>();
</script>

<script lang="ts">
  import Icon from './Icon.svelte';
  import MediaTimeline from './MediaTimeline.svelte';
  import { audioPreview, measureAudioLoudness } from './arcade';
  import { bytes, clock } from './media-format';
  import type { SelectedFile, ToolSummary } from './contracts';
  import type { UiValues } from './standard-ui';

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

  const COLUMNS = 240;
  const toolId = $derived(tool.id);
  const audioFiles = $derived(files.filter((file) => file.mime === 'file/audio'));
  const source = $derived(audioFiles[0] ?? null);
  const kind = $derived.by(() => {
    switch (toolId) {
      case 'arcade.audio.trim': return 'trim';
      case 'arcade.audio.silence': return 'silence';
      case 'arcade.audio.normalize': return 'loudness';
      case 'arcade.audio.speed-pitch': return 'speed';
      case 'arcade.audio.metadata': return 'tags';
      case 'arcade.audio.join': return 'join';
      default: return 'summary';
    }
  });

  function number(key: string, fallback: number): number {
    const value = Number(values[key]);
    return values[key] !== '' && values[key] !== undefined && Number.isFinite(value) ? value : fallback;
  }

  // Silence settings are re-analysed after typing settles.
  let silenceSettings = $state<{ thresholdDb: number; minimumSeconds: number } | null>(null);
  $effect(() => {
    if (kind !== 'silence') { silenceSettings = null; return; }
    const next = { thresholdDb: number('thresholdDb', -45), minimumSeconds: number('minimumSeconds', 0.5) };
    if (!silenceSettings) { silenceSettings = next; return; }
    if (next.thresholdDb === silenceSettings.thresholdDb && next.minimumSeconds === silenceSettings.minimumSeconds) return;
    const timer = setTimeout(() => { silenceSettings = next; }, 350);
    return () => clearTimeout(timer);
  });

  function requestPreview(token: string, columns: number, silence: typeof silenceSettings): Promise<AudioPreview> {
    const key = `${token}:${columns}:${silence ? `${silence.thresholdDb}/${silence.minimumSeconds}` : ''}`;
    let request = previewCache.get(key);
    if (!request) {
      request = audioPreview(token, columns, silence ?? undefined).catch((error) => {
        previewCache.delete(key);
        throw error;
      });
      previewCache.set(key, request);
    }
    return request;
  }

  let preview = $state<AudioPreview | null>(null);
  let previewError = $state('');
  let loading = $state(false);

  $effect(() => {
    const token = source?.token;
    if (!token) { preview = null; return; }
    if (kind === 'silence' && !silenceSettings) return;
    const columns = kind === 'trim' || kind === 'silence' ? COLUMNS : 0;
    let current = true;
    loading = true;
    previewError = '';
    requestPreview(token, columns, kind === 'silence' ? silenceSettings : null)
      .then((value) => { if (current) preview = value; })
      .catch((error) => { if (current) { preview = null; previewError = typeof error === 'string' ? error : 'Could not read this audio file.'; } })
      .finally(() => { if (current) loading = false; });
    return () => { current = false; };
  });

  const duration = $derived(preview?.durationSeconds ?? 0);

  function channelLabel(count: number | null, layout: string | null): string {
    if (count === 1) return 'mono';
    if (count === 2) return 'stereo';
    return layout ?? (count ? `${count} channels` : '');
  }

  // ---- Waveform -------------------------------------------------------------

  /** Bars on a dB scale so quiet passages stay visible. */
  const wavePath = $derived.by(() => {
    const peaks = preview?.peaks ?? [];
    let path = '';
    peaks.forEach((peak, index) => {
      const db = peak > 0 ? 20 * Math.log10(peak) : -90;
      const height = Math.max(1.5, Math.min(48, ((db + 60) / 60) * 48));
      path += `M${index + 0.5} ${(50 - height).toFixed(1)}V${(50 + height).toFixed(1)}`;
    });
    return path;
  });

  // ---- Trim ---------------------------------------------------------------

  const removing = $derived(values.mode === 'remove');
  const rangeStart = $derived(Math.min(number('startSeconds', 0), duration || Infinity));
  const rangeEnd = $derived(Math.min(number('endSeconds', duration), duration || Infinity));

  function write(key: string, seconds: number): void {
    onValueChange(key, String(Math.round(seconds * 100) / 100));
  }

  function setHandle(handle: 'start' | 'end' | 'point', seconds: number): void {
    if (handle === 'start') write('startSeconds', Math.max(0, Math.min(seconds, rangeEnd - 0.05)));
    else if (handle === 'end') write('endSeconds', Math.min(duration, Math.max(seconds, rangeStart + 0.05)));
  }

  function moveRange(start: number): void {
    const length = rangeEnd - rangeStart;
    write('startSeconds', start);
    write('endSeconds', start + length);
  }

  const trimCopies = $derived(kind === 'trim' && !removing && (values.format ?? 'same') === 'same'
    && number('fadeInSeconds', 0) === 0 && number('fadeOutSeconds', 0) === 0 && values.precise !== 'true' && (preview?.copyable ?? false));

  // ---- Silence --------------------------------------------------------------

  const silences = $derived(preview?.silences ?? []);
  const silencePlan = $derived.by(() => {
    if (kind !== 'silence' || !preview?.silences || !duration) return null;
    const operation = values.operation ?? 'trim';
    if (operation === 'trim') {
      const pad = number('paddingSeconds', 0.1);
      const lead = silences[0] && silences[0][0] <= 0.01 ? Math.max(0, silences[0][1] - pad) : 0;
      const last = silences[silences.length - 1];
      const trail = last && last[1] >= duration - 0.01 ? Math.max(0, duration - last[0] - pad) : 0;
      return lead || trail
        ? `Removes ${lead.toFixed(1)} s at the start and ${trail.toFixed(1)} s at the end`
        : 'No silence at the start or end with these settings';
    }
    if (operation === 'shorten') {
      const keep = number('keepSeconds', 0.3);
      const removed = silences.reduce((sum, [from, to]) => sum + Math.max(0, to - from - keep), 0);
      return silences.length ? `Shortens ${silences.length} pause${silences.length === 1 ? '' : 's'}, removing about ${removed.toFixed(1)} s` : 'No pauses found with these settings';
    }
    const inner = silences.filter(([from, to]) => from > 0.01 && to < duration - 0.01).length;
    return inner ? `Splits into about ${inner + 1} parts` : 'No silence separates parts with these settings';
  });
  const silentSeconds = $derived(silences.reduce((sum, [from, to]) => sum + (to - from), 0));

  // ---- Loudness -------------------------------------------------------------

  let loudness = $state<LoudnessReport | null>(null);
  let measuring = $state(false);
  let loudnessError = $state('');

  $effect(() => {
    // A different file invalidates the measurement.
    void source?.token;
    loudness = null;
    loudnessError = '';
  });

  $effect(() => {
    // Short files are measured right away; long ones wait for the button.
    if (kind === 'loudness' && source && duration > 0 && duration <= 600 && !loudness && !measuring && !loudnessError) void measure();
  });

  async function measure(): Promise<void> {
    if (!source) return;
    const token = source.token;
    measuring = true;
    loudnessError = '';
    try {
      let request = loudnessCache.get(token);
      if (!request) {
        request = measureAudioLoudness(token).catch((error) => { loudnessCache.delete(token); throw error; });
        loudnessCache.set(token, request);
      }
      const value = await request;
      if (source?.token === token) loudness = value;
    } catch (error) {
      if (source?.token === token) loudnessError = typeof error === 'string' ? error : 'Could not measure loudness.';
    } finally {
      measuring = false;
    }
  }

  const loudnessTarget = $derived.by(() => {
    if (values.mode === 'peak') return null;
    switch (values.preset ?? 'podcast') {
      case 'streaming': return -14;
      case 'podcast': return -16;
      case 'speech': return -18;
      case 'broadcast': return -23;
      default: return number('targetLufs', -16);
    }
  });
  const gainNeeded = $derived.by(() => {
    if (!loudness) return null;
    if (values.mode === 'peak') return loudness.truePeak === null ? null : number('peakDb', -1) - loudness.truePeak;
    return loudness.integrated === null || loudnessTarget === null ? null : loudnessTarget - loudness.integrated;
  });

  function signed(value: number, unit: string): string {
    return `${value > 0 ? '+' : value < 0 ? '−' : '±'}${Math.abs(value).toFixed(1)} ${unit}`;
  }

  function level(value: number | null, unit: string): string {
    return value === null ? 'silent' : `${value < 0 ? '−' : ''}${Math.abs(value).toFixed(1)} ${unit}`;
  }

  // ---- Speed --------------------------------------------------------------

  const speed = $derived(Math.max(0.25, Math.min(4, number('speed', 1))));
  const pitchChange = $derived(values.preservePitch === 'false' ? 12 * Math.log2(speed) : number('pitchSemitones', 0));

  // ---- Tags ---------------------------------------------------------------

  const TAG_FIELDS: Array<[string, string]> = [
    ['title', 'title'], ['artist', 'artist'], ['album', 'album'], ['albumArtist', 'album_artist'], ['track', 'track'],
    ['disc', 'disc'], ['year', 'date'], ['genre', 'genre'], ['composer', 'composer'], ['comment', 'comment'],
  ];
  const prefilled = new Set<string>();
  const newCover = $derived(files.find((file) => file.mime === 'file/image') ?? null);

  $effect(() => {
    // Editing starts from the file's current tags; only empty fields are filled.
    if (kind !== 'tags' || values.operation !== 'edit' || !preview || !source) return;
    if (prefilled.has(source.token)) return;
    prefilled.add(source.token);
    for (const [field, tag] of TAG_FIELDS) {
      let value = preview.tags[tag] ?? (tag === 'date' ? preview.tags.year : undefined);
      if (!value || (values[field] ?? '') !== '') continue;
      if (field === 'year' && !/^\d{4}(-\d{2}-\d{2})?$/.test(value)) value = value.slice(0, 4);
      onValueChange(field, value);
    }
  });

  const shownTags = $derived(Object.entries(preview?.tags ?? {}).filter(([key]) => !['encoder', 'major_brand', 'minor_version', 'compatible_brands'].includes(key)));

  // ---- Join ---------------------------------------------------------------

  let clips = $state<AudioPreview[] | null>(null);
  $effect(() => {
    if (kind !== 'join' || audioFiles.length < 2) { clips = null; return; }
    let current = true;
    Promise.all(audioFiles.map((file) => requestPreview(file.token, 0, null)))
      .then((items) => { if (current) clips = items; })
      .catch(() => { if (current) clips = null; });
    return () => { current = false; };
  });
  const joinPlan = $derived.by(() => {
    if (!clips?.length) return null;
    const total = clips.reduce((sum, clip) => sum + (clip.durationSeconds ?? 0), 0);
    const joins = clips.length - 1;
    const crossfade = number('crossfadeSeconds', 0);
    const gap = number('gapSeconds', 0);
    const first = clips[0];
    const mismatch = clips.find((clip) => clip.codec !== first.codec || clip.sampleRate !== first.sampleRate || clip.channels !== first.channels);
    const lossless = !mismatch && first.copyable && (values.format ?? 'same') === 'same' && crossfade === 0 && gap === 0;
    return {
      length: total + gap * joins - crossfade * joins,
      lossless,
      reason: mismatch ? 'The clips use different formats, so they will be re-encoded.' : '',
    };
  });
</script>

{#if source}
  <section class="audio-assist" aria-label="Audio preview" aria-busy={loading}>
    {#if previewError}
      <p class="assist-error" role="alert">{previewError}</p>
    {:else if preview}
      <div class="assist-meta">
        {#if preview.codec}<span>{preview.codec}</span>{/if}
        {#if preview.sampleRate}<span>{preview.sampleRate / 1000} kHz</span>{/if}
        {#if preview.channels}<span>{channelLabel(preview.channels, preview.channelLayout)}</span>{/if}
        {#if preview.bitRate && !preview.lossless}<span>{Math.round(preview.bitRate / 1000)} kb/s</span>{/if}
        {#if duration}<span>{clock(duration)}</span>{/if}
        <span>{bytes(preview.sourceBytes)}</span>
        {#if preview.lossless}<span class="chip">Lossless</span>{/if}
        {#if audioFiles.length > 1 && kind !== 'join'}<span class="assist-note">Showing the first of {audioFiles.length} files</span>{/if}
      </div>

      {#if kind === 'trim' && duration > 0}
        <MediaTimeline
          {duration}
          mode="range"
          start={rangeStart}
          end={rangeEnd}
          step={0.01}
          invert={removing}
          {disabled}
          label="Waveform"
          onSeek={setHandle}
          onMove={moveRange}
        >
          {#snippet background()}
            <svg class="waveform" viewBox={`0 0 ${Math.max(1, preview!.peaks.length)} 100`} preserveAspectRatio="none" aria-hidden="true"><path d={wavePath} /></svg>
          {/snippet}
        </MediaTimeline>
        <div class="timeline-readout">
          <span>{clock(rangeStart)}</span>
          <strong>{(rangeEnd - rangeStart).toFixed(2)} s {removing ? 'removed' : 'kept'}</strong>
          <span>{clock(rangeEnd)}</span>
        </div>
        <p class="assist-note">{trimCopies ? 'Saved without re-encoding, in the original quality.' : removing ? `Leaves ${clock(Math.max(0, duration - (rangeEnd - rangeStart)))}; the cut is joined with a 10 ms crossfade.` : 'The selection is re-encoded.'} Arrow keys move a handle 10 ms · Shift moves 1 s.</p>
      {/if}

      {#if kind === 'silence' && duration > 0}
        <div class="wave-static" class:busy={loading}>
          <svg class="waveform" viewBox={`0 0 ${Math.max(1, preview.peaks.length)} 100`} preserveAspectRatio="none" aria-hidden="true"><path d={wavePath} /></svg>
          {#each silences as [from, to], index (index)}
            <div class="region" style:left={`${(from / duration) * 100}%`} style:width={`${((to - from) / duration) * 100}%`}></div>
          {/each}
        </div>
        <div class="timeline-readout">
          <span>{silences.length} silent stretch{silences.length === 1 ? '' : 'es'} · {silentSeconds.toFixed(1)} s</span>
          {#if silencePlan}<strong>{silencePlan}</strong>{/if}
        </div>
      {/if}

      {#if kind === 'loudness'}
        <div class="meter-row">
          {#if loudness}
            <div class="meter"><span>Integrated</span><strong>{level(loudness.integrated, 'LUFS')}</strong></div>
            <div class="meter"><span>True peak</span><strong>{level(loudness.truePeak, 'dBTP')}</strong></div>
            <div class="meter"><span>Range</span><strong>{loudness.range === null ? '—' : `${loudness.range.toFixed(1)} LU`}</strong></div>
            {#if gainNeeded !== null}<div class="meter accent"><span>Change</span><strong>{signed(gainNeeded, 'dB')}</strong></div>{/if}
          {:else if loudnessError}
            <span class="assist-error" role="alert">{loudnessError}</span>
          {:else if measuring}
            <span class="assist-loading"><span class="spinner"></span><span>Measuring loudness…</span></span>
          {:else}
            <span class="assist-note">Measure the current loudness before you run.</span>
          {/if}
          {#if !loudness && !measuring}
            <button type="button" class="quiet-button" onclick={() => void measure()} disabled={disabled}><Icon name="spark" size={14} /><span>Measure</span></button>
          {/if}
        </div>
        {#if audioFiles.length > 1}<p class="assist-note">Each file is measured and adjusted on its own.</p>{/if}
      {/if}

      {#if kind === 'speed' && duration > 0}
        <div class="timeline-readout">
          <span>{clock(duration)} → <strong>{clock(duration / speed)}</strong></span>
          <span>{Math.abs(pitchChange) < 0.05 ? 'Pitch unchanged' : `Pitch ${pitchChange > 0 ? 'up' : 'down'} ${Math.abs(pitchChange).toFixed(1)} semitones`}</span>
        </div>
      {/if}

      {#if kind === 'tags'}
        <div class="tag-card">
          {#if newCover}
            <div class="cover placeholder" title="New cover selected"><Icon name="image" size={18} /></div>
          {:else if preview.cover}
            <img class="cover" src={preview.cover} alt="Current cover art" />
          {:else}
            <div class="cover placeholder" title="No cover art"><Icon name="audio" size={18} /></div>
          {/if}
          <div class="tag-list">
            {#if shownTags.length}
              {#each shownTags.slice(0, 8) as [key, value] (key)}
                <div><span>{key.replaceAll('_', ' ')}</span><strong>{value}</strong></div>
              {/each}
              {#if shownTags.length > 8}<p class="assist-note">+{shownTags.length - 8} more</p>{/if}
            {:else}
              <p class="assist-note">This file has no tags yet.</p>
            {/if}
          </div>
        </div>
        {#if values.operation === 'edit'}
          <p class="assist-note">{newCover ? `${newCover.name} will become the cover. ` : 'Add a JPEG or PNG to replace the cover. '}Fields start with the current tags; nothing is re-encoded.</p>
        {/if}
      {/if}

      {#if kind === 'join' && joinPlan}
        <div class="timeline-readout">
          <span>{audioFiles.length} clips · {clock(joinPlan.length)} total</span>
          <strong>{joinPlan.lossless ? 'Joins without re-encoding' : 'Re-encodes'}</strong>
        </div>
        {#if joinPlan.reason && (values.format ?? 'same') === 'same'}<p class="assist-note">{joinPlan.reason}</p>{/if}
      {/if}
    {:else}
      <div class="assist-loading"><span class="spinner"></span><span>Reading audio…</span></div>
    {/if}
  </section>
{/if}

<style>
  .audio-assist { display: grid; gap: 10px; margin: 12px 14px 0; }
  .assist-meta { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 12px; color: var(--ink-3); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .chip { padding: 1px 6px; border-radius: 6px; color: var(--mint); background: var(--mint-wash); font-size: 10.5px; font-weight: 600; }
  .assist-note { margin: 0; color: var(--ink-3); font-size: 11.5px; line-height: 1.45; }
  .assist-error { margin: 0; color: var(--red); font-size: 12px; line-height: 1.45; }
  .assist-loading { display: flex; align-items: center; gap: 8px; min-height: 40px; color: var(--ink-3); font-size: 12px; }

  .waveform { width: 100%; height: 100%; }
  .waveform path { fill: none; stroke: var(--ink-2); stroke-width: .62; opacity: .75; }
  .wave-static { position: relative; height: 52px; overflow: hidden; border-radius: 9px; background: var(--field-bg); transition: opacity 120ms ease; }
  .wave-static.busy { opacity: .6; }
  .region { position: absolute; top: 0; bottom: 0; background: var(--amber-wash); box-shadow: inset 0 -2px 0 color-mix(in srgb, var(--amber) 70%, transparent); }
  .timeline-readout { display: flex; align-items: center; justify-content: space-between; gap: 10px; min-height: 22px; color: var(--ink-3); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .timeline-readout strong { color: var(--ink); font-weight: 550; }

  .meter-row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-height: 44px; padding: 6px 8px; border-radius: 10px; background: var(--surface-1); }
  .meter { display: grid; gap: 1px; min-width: 84px; padding: 2px 6px; }
  .meter span { color: var(--ink-3); font-size: 11px; }
  .meter strong { color: var(--ink); font-size: 13.5px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .meter.accent strong { color: var(--primary); }
  .meter-row .quiet-button { margin-left: auto; }

  .tag-card { display: flex; gap: 12px; padding: 10px; border-radius: 10px; background: var(--surface-1); }
  .cover { width: 72px; height: 72px; flex: 0 0 auto; border-radius: 8px; object-fit: cover; }
  .cover.placeholder { display: grid; place-items: center; color: var(--ink-3); background: var(--surface-2); }
  .tag-list { display: grid; min-width: 0; flex: 1; gap: 3px; align-content: start; }
  .tag-list div { display: flex; gap: 10px; min-width: 0; font-size: 12px; }
  .tag-list span { width: 92px; flex: 0 0 auto; color: var(--ink-3); text-transform: capitalize; }
  .tag-list strong { overflow: hidden; color: var(--ink); font-weight: 500; text-overflow: ellipsis; white-space: nowrap; }
</style>
