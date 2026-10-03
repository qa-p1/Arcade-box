<script lang="ts">
  // Generic view for calculator-style results: a large headline, an optional
  // one-line summary, the remaining facts, and any tables below.
  import ResultData from './ResultData.svelte';
  import { displayValue, resultLabel } from './result-presentation';

  let { value }: { value: unknown; mime?: string; copy?: (text: string) => void } = $props();
  const report = $derived((value && typeof value === 'object' ? value : {}) as Record<string, unknown>);
  const hidden = new Set(['headline', 'summary', 'input', 'value', 'result']);
  const facts = $derived(Object.entries(report).filter(([key, item]) => !hidden.has(key) && item !== null && typeof item !== 'object'));
  const nested = $derived(Object.entries(report).filter(([key, item]) => !hidden.has(key) && item !== null && typeof item === 'object'));

  function fact(item: unknown): string {
    return typeof item === 'number' && !Number.isInteger(item) ? String(Math.round(item * 100) / 100) : displayValue(item);
  }
</script>

<div class="headline-report">
  <p class="headline">{displayValue(report.headline)}</p>
  {#if typeof report.summary === 'string'}<p class="summary">{report.summary}</p>{/if}
  {#if facts.length}
    <dl>{#each facts as [key, item] (key)}<dt>{resultLabel(key)}</dt><dd>{fact(item)}</dd>{/each}</dl>
  {/if}
  {#each nested as [key, item] (key)}
    <section><h4>{resultLabel(key)}</h4><ResultData value={item} /></section>
  {/each}
</div>

<style>
  .headline-report { display: grid; gap: 10px; }
  .headline { margin: 0; font-size: 22px; font-weight: 650; line-height: 1.3; color: var(--ink); overflow-wrap: anywhere; user-select: text; }
  .summary { margin: -4px 0 0; color: var(--ink-2); font-size: 13px; }
  dl { display: grid; grid-template-columns: minmax(110px, max-content) 1fr; gap: 6px 16px; margin: 4px 0 0; font-size: 12.5px; }
  dt { color: var(--ink-3); }
  dd { margin: 0; min-width: 0; overflow-wrap: anywhere; color: var(--ink); user-select: text; }
  h4 { margin: 6px 0 8px; font-size: 12px; font-weight: 600; color: var(--ink-3); }
</style>
