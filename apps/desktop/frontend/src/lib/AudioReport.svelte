<script lang="ts">
  // Readable views of the audio tools' structured results: loudness before and
  // after normalization, and a file's tags and format.
  import { clock } from './media-format';

  let { mime, value }: { mime: string; value: unknown; copy?: (text: string) => void } = $props();

  type Levels = { integrated?: number | null; truePeak?: number | null; range?: number | null; peak?: number | null };
  const report = $derived((value && typeof value === 'object' ? value : {}) as Record<string, unknown>);
  const before = $derived((report.before ?? {}) as Levels);
  const after = $derived((report.after ?? {}) as Levels);
  const target = $derived((report.target ?? {}) as Levels);
  const tags = $derived(Object.entries((report.tags ?? {}) as Record<string, string>));

  function level(value: number | null | undefined, unit: string): string {
    return typeof value === 'number' ? `${value < 0 ? '−' : ''}${Math.abs(value).toFixed(1)} ${unit}` : '—';
  }

  function format(): string {
    const parts: string[] = [];
    if (typeof report.codec === 'string') parts.push(report.codec);
    if (typeof report.sampleRate === 'number') parts.push(`${report.sampleRate / 1000} kHz`);
    if (report.channels === 1) parts.push('mono');
    else if (report.channels === 2) parts.push('stereo');
    else if (typeof report.channelLayout === 'string') parts.push(report.channelLayout);
    if (typeof report.bitRate === 'number') parts.push(`${Math.round(report.bitRate / 1000)} kb/s`);
    if (typeof report.durationSeconds === 'number') parts.push(clock(report.durationSeconds));
    return parts.join(' · ');
  }
</script>

{#if mime === 'structured/audio-loudness'}
  <div class="media-facts">
    {#if report.mode === 'peak'}
      <div><span>Peak before</span><strong>{level(before.peak, 'dBFS')}</strong></div>
      <div><span>Peak after</span><strong>{level(after.peak, 'dBFS')}</strong></div>
      <div><span>Gain</span><strong>{typeof report.gainDb === 'number' ? `${report.gainDb > 0 ? '+' : ''}${report.gainDb.toFixed(1)} dB` : '—'}</strong></div>
    {:else}
      <div><span>Before</span><strong>{level(before.integrated, 'LUFS')}</strong></div>
      <div><span>After</span><strong>{level(after.integrated, 'LUFS')}</strong></div>
      <div><span>True peak after</span><strong>{level(after.truePeak, 'dBTP')}</strong></div>
      <div><span>Target</span><strong>{level(target.integrated, 'LUFS')}{report.method === 'dynamic' ? ' · dynamic' : ''}</strong></div>
    {/if}
  </div>
{:else}
  <div class="audio-tags">
    {#if format()}<p>{format()}{report.cover ? ' · cover art' : ''}</p>{/if}
    {#if tags.length}
      <dl>
        {#each tags as [key, text] (key)}<dt>{key.replaceAll('_', ' ')}</dt><dd>{text}</dd>{/each}
      </dl>
    {:else}
      <p>No tags.</p>
    {/if}
  </div>
{/if}

<style>
  .audio-tags { display: grid; gap: 8px; }
  .audio-tags p { margin: 0; color: var(--ink-3); font-size: 12px; }
  dl { display: grid; grid-template-columns: minmax(90px, max-content) 1fr; gap: 4px 14px; margin: 0; font-size: 12.5px; }
  dt { color: var(--ink-3); text-transform: capitalize; }
  dd { margin: 0; overflow-wrap: anywhere; color: var(--ink); }
</style>
