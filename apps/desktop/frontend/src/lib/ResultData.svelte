<script lang="ts">
  import { displayValue, resultLabel, tableColumns, RESULT_PAGE_SIZE } from './result-presentation';

  let { value, depth = 0 }: { value: unknown; depth?: number } = $props();
  let page = $state(0);
  const rows = $derived(Array.isArray(value) ? value : null);
  const entries = $derived(!rows && value !== null && typeof value === 'object' ? Object.entries(value) : []);
  const count = $derived(rows?.length ?? entries.length);
  const lastPage = $derived(Math.max(0, Math.ceil(count / RESULT_PAGE_SIZE) - 1));
  const currentPage = $derived(Math.min(page, lastPage));
  const offset = $derived(currentPage * RESULT_PAGE_SIZE);
  const columns = $derived(rows ? tableColumns(rows.slice(offset, offset + RESULT_PAGE_SIZE)) : []);
</script>

{#snippet child(item: unknown)}
  {#if item !== null && typeof item === 'object'}
    {#if depth < 5}
      <details class="nested">
        <summary>{Array.isArray(item) ? `${item.length} items` : `${Object.keys(item).length} fields`}</summary>
        <!-- Svelte recursion is bounded in depth and each level paginates independently. -->
        <LazyResultData value={item} depth={depth + 1} />
      </details>
    {:else}<span>{displayValue(item).slice(0, 300)}{displayValue(item).length > 300 ? '…' : ''}</span>{/if}
  {:else}{@const text = displayValue(item)}<span class="value">{text.slice(0, 2000)}{#if text.length > 2000}… <small>(copy result for full value)</small>{/if}</span>{/if}
{/snippet}

{#if rows}
  {#if !rows.length}<p class="empty">No items.</p>
  {:else if columns.length}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex (scrollable results need keyboard access) -->
    <div class="table-scroll" tabindex="0" role="region" aria-label="Result table">
      <table><thead><tr>{#each columns as column}<th scope="col">{resultLabel(column)}</th>{/each}</tr></thead>
        <tbody>{#each rows.slice(offset, offset + RESULT_PAGE_SIZE) as row}<tr>{#each columns as column}<td>{@render child(row[column])}</td>{/each}</tr>{/each}</tbody>
      </table>
    </div>
  {:else}
    <ol start={offset + 1}>{#each rows.slice(offset, offset + RESULT_PAGE_SIZE) as row}<li>{@render child(row)}</li>{/each}</ol>
  {/if}
{:else if entries.length}
  <dl>{#each entries.slice(offset, offset + RESULT_PAGE_SIZE) as [key, item]}<div><dt>{resultLabel(key)}</dt><dd>{@render child(item)}</dd></div>{/each}</dl>
{:else}<span class="value">{displayValue(value)}</span>{/if}

{#if count > RESULT_PAGE_SIZE}
  <nav aria-label="Result pages"><button disabled={currentPage === 0} onclick={() => (page = currentPage - 1)}>Previous</button><span>{offset + 1}–{Math.min(offset + RESULT_PAGE_SIZE, count)} of {count}</span><button disabled={currentPage >= lastPage} onclick={() => (page = currentPage + 1)}>Next</button></nav>
{/if}

<script lang="ts" module>
  import LazyResultData from './LazyResultData.svelte';
</script>

<style>
  .table-scroll { max-width: 100%; overflow: auto; max-height: 360px; border: 1px solid var(--line); border-radius: 8px; }
  table { border-collapse: collapse; width: 100%; font-size: 13px; }
  th { text-align: left; font-weight: 600; color: var(--muted); background: var(--surface-raised); position: sticky; top: 0; }
  th, td { padding: 9px 12px; border-bottom: 1px solid var(--line); min-width: 90px; max-width: 280px; overflow-wrap: anywhere; vertical-align: top; }
  dl { margin: 0; display: grid; gap: 0; }
  dl > div { display: grid; grid-template-columns: minmax(110px, .65fr) minmax(0, 1fr); gap: 16px; padding: 9px 0; border-bottom: 1px solid var(--line); }
  dt { color: var(--muted); font-size: 13px; overflow-wrap: anywhere; }
  dd { margin: 0; min-width: 0; font-size: 13px; }
  .value { white-space: pre-wrap; overflow-wrap: anywhere; line-height: 1.6; }
  .nested { min-width: 0; }
  summary { color: var(--muted); cursor: pointer; padding: 2px 0; font-size: 13px; }
  ol { padding-left: 24px; margin: 0; }
  li { padding: 5px 0; overflow-wrap: anywhere; }
  nav { display: flex; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: 10px; padding-top: 12px; font-size: 12px; color: var(--muted); }
  button { border: 1px solid var(--line); border-radius: 6px; padding: 6px 10px; background: var(--surface-raised); cursor: pointer; }
  button:disabled { opacity: .4; cursor: default; }
  .empty { color: var(--muted); margin: 0; font-size: 13px; }
  @media (max-width: 480px) { dl > div { grid-template-columns: minmax(0, 1fr); gap: 4px; } }
</style>
