<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { ToolOutput } from './contracts';
  import { copyText } from './arcade';
  import Icon from './Icon.svelte';
  import type { Component } from 'svelte';
  import AudioReport from './AudioReport.svelte';
  import ColorReport from './ColorReport.svelte';
  import EmojiReport from './EmojiReport.svelte';
  import HeadlineReport from './HeadlineReport.svelte';
  import ResultData from './ResultData.svelte';
  import SiteCheckReport from './SiteCheckReport.svelte';
  import { parseResultData, TEXT_PREVIEW_LIMIT } from './result-presentation';

  type ReportProps = { value: unknown; mime: string; copy: (text: string) => void };
  // Readable views for structured results, by output type. Anything else with
  // a `headline` uses the calculator-style view, then the generic data view.
  const reports: Record<string, Component<ReportProps>> = {
    'structured/audio-loudness': AudioReport as Component<ReportProps>,
    'structured/audio-metadata': AudioReport as Component<ReportProps>,
    'structured/site-check': SiteCheckReport as Component<ReportProps>,
    'structured/color': ColorReport as Component<ReportProps>,
    'structured/emoji-list': EmojiReport as Component<ReportProps>,
  };

  let { output, toolId }: { output: ToolOutput; toolId: string } = $props();
  let copied = $state(false);
  let error = $state('');
  let timer: ReturnType<typeof setTimeout> | undefined;
  const data = $derived(parseResultData(output.value, output.mime, toolId));
  const Report = $derived(data === undefined ? undefined : reports[output.mime] ?? (typeof (data as { headline?: unknown }).headline === 'string' ? HeadlineReport as Component<ReportProps> : undefined));
  const preview = $derived(output.value.slice(0, TEXT_PREVIEW_LIMIT));
  const truncated = $derived(output.value.length > TEXT_PREVIEW_LIMIT);
  const palette = $derived.by(() => {
    if (output.mime !== 'structured/color-palette' || !data || typeof data !== 'object' || !('colors' in data) || !Array.isArray(data.colors)) return [];
    return data.colors.filter((color): color is { hex: string } => typeof color?.hex === 'string' && /^#[0-9a-f]{6}$/i.test(color.hex)).slice(0, 32);
  });
  async function copy(value = output.value): Promise<void> {
    error = '';
    try { await copyText(value); copied = true; clearTimeout(timer); timer = setTimeout(() => (copied = false), 1800); }
    catch (reason) { error = typeof reason === 'string' ? reason : 'Could not copy. Try again.'; }
  }
  onDestroy(() => clearTimeout(timer));
</script>

<div class="tool-output-view">
  {#if Report}
    <Report value={data} mime={output.mime} copy={(text) => void copy(text)} />
  {:else if palette.length}
    <div class="palette" aria-label="Extracted colors">{#each palette as color}<button title={`Copy ${color.hex}`} onclick={() => void copy(color.hex)}><span style:background={color.hex}></span><code>{color.hex}</code></button>{/each}</div>
  {:else if data !== undefined}
    <ResultData value={data} />
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex (keyboard scrolling for long results) -->
    <pre class="text-preview" tabindex="0" aria-label="Result">{preview}</pre>
  {/if}
  {#if data !== undefined}<details class="raw"><summary>Raw preview</summary><!-- svelte-ignore a11y_no_noninteractive_tabindex (keyboard scrolling for long results) --><pre class="text-preview" tabindex="0" aria-label="Raw result">{preview}</pre></details>{/if}
  {#if truncated}<p class="preview-note">Preview limited to {TEXT_PREVIEW_LIMIT.toLocaleString()} characters. Copy includes the complete result.</p>{/if}
  <div class="output-actions"><button class="quiet-button" onclick={() => void copy()}><Icon name={copied ? 'check' : 'copy'} size={14} /><span>{copied ? 'Copied' : 'Copy result'}</span></button></div>
  {#if error}<p class="field-error" role="alert">{error}</p>{/if}
</div>

<style>
  .tool-output-view { padding: 14px; min-width: 0; }
  .text-preview { margin: 0; max-height: 340px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; font: 13px/1.65 ui-monospace, monospace; tab-size: 2; }
  .output-actions { display: flex; justify-content: flex-end; margin-top: 12px; }
  .raw { margin-top: 12px; }
  summary { color: var(--muted); cursor: pointer; font-size: 12px; padding: 6px 0; }
  .preview-note { font-size: 12px; color: var(--muted); }
  .palette { display: grid; grid-template-columns: repeat(auto-fit, minmax(85px, 1fr)); gap: 10px; }
  .palette button { display: grid; gap: 8px; padding: 0 0 8px; border: 1px solid var(--line); border-radius: 8px; overflow: hidden; background: var(--surface-raised); cursor: pointer; }
  .palette span { height: 64px; }
  code { font-size: 12px; }
</style>
