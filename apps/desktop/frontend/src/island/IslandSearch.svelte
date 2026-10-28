<script lang="ts">
  import { arcadeAppName } from '../lib/arcade';
  import Icon from '../lib/Icon.svelte';
  import { chordParts } from '../lib/chords';
  import { privacyLabel } from '../lib/tool-utils';
  import boxGlyph from '../design/glyphs/arcade.box.svg';
  import { useBox } from '../box.svelte';

  const box = useBox();
  const shortcut = $derived(box.shortcutInfo?.triggerDescription || box.shortcutInfo?.trigger_description || '');
</script>
{#if box.pendingLink}
  <div class="link-pending" role="status">
    <span>From {arcadeAppName(box.pendingLink.source)}: {box.pendingLink.files.length ? `${box.pendingLink.files.length === 1 ? box.pendingLink.files[0].name : `${box.pendingLink.files.length} files`}` : 'text'}. Choose a tool to use it.</span>
    <button type="button" class="btn ghost icon-only sm" aria-label="Discard the input from the other app" onclick={() => (box.pendingLink = null)}><Icon name="close" size={14} /></button>
  </div>
{/if}
<div class="overlay-head island-search" class:has-query={box.query.trim().length > 0}>
  <span class="brand-mark island-mark" aria-hidden="true"><img src={boxGlyph} alt="" width="16" height="16" /></span>
  <input
    bind:this={box.searchInput}
    bind:value={box.query}
    onfocus={() => (box.focused = true)}
    onblur={(event) => {
      const nextFocus = event.relatedTarget;
      box.focused = nextFocus instanceof Node && Boolean(box.islandShell?.contains(nextFocus));
    }}
    onkeydown={box.handleSearchKeydown}
    aria-label="Search tools"
    aria-describedby="search-keyboard-help"
    aria-keyshortcuts="ArrowDown ArrowUp Enter Escape"
    autocomplete="off"
    autocapitalize="off"
    spellcheck="false"
    placeholder="What do you need done?"
  />
  {#if box.query}
    <button type="button" class="btn ghost icon-only sm" aria-label="Clear search" onclick={() => { box.query = ''; box.searchInput?.focus(); }}><Icon name="close" size={14} /></button>
  {:else}
    {#if shortcut}<span class="key-chip-list shortcut-hint" title="Shows or hides the Island from any app" aria-hidden="true">{#each chordParts(shortcut) as part, i (`${part}-${i}`)}<kbd class="key-chip">{part}</kbd>{/each}</span>{/if}
    <button type="button" class="btn ghost icon-only sm" aria-label="Open dashboard" title="Open dashboard" onclick={box.toggleDashboard}><Icon name="apps" size={15} /></button>
  {/if}
</div>

{#if box.query.trim()}
  <div class="overlay-body search-panel" id="search-list" role="listbox" aria-label="Tools">
    {#if box.searchError}
      <div class="notice danger island-notice" role="alert"><Icon name="alert" /><span class="row-main">Couldn't search the tools. {box.searchError}</span></div>
    {:else if box.runnableSearchResults.length === 0 && box.loadingSearch}
      {#each [0, 1, 2] as item (item)}<div class="skeleton-row"><span class="glyph-tile sm skeleton"></span><div class="row-main"><div class="skeleton line"></div></div></div>{/each}
    {:else if box.runnableSearchResults.length === 0}
      <div class="island-empty"><Icon name="search" size={20} /><strong>No tools match “{box.query.trim()}”</strong><span>Try another word, or open the dashboard to browse.</span></div>
    {:else}
      {#each box.runnableSearchResults as tool, index (tool.id)}
        {#if index === 0 || box.runnableSearchResults[index - 1].category !== tool.category}
          <div class="t-caption group-label">{tool.category}</div>
        {/if}
        <div class="result-row">
          <button
            type="button"
            id={`tool-${tool.id}`}
            role="option"
            class="overlay-row two-line"
            aria-selected={index === box.selectedIndex}
            onclick={() => box.openTool(tool)}
            onmouseenter={() => (box.selectedIndex = index)}
          >
            <span class="glyph-tile sm"><Icon name={box.iconForCategory(tool.category)} size={14} /></span>
            <span class="row-main"><span class="row-title"><span class="ellipsis">{tool.name}</span>{#if privacyLabel(tool.privacyClass) !== 'LOCAL'}<span class="badge warn">online</span>{/if}</span><span class="row-sub ellipsis">{tool.description}</span></span>
            {#if index === box.selectedIndex}<kbd class="key-chip" aria-hidden="true">↵</kbd>{/if}
          </button>
          <button type="button" class="btn ghost icon-only sm favorite" class:on={box.isFavorite(tool.id)} aria-label={box.isFavorite(tool.id) ? `Remove ${tool.name} from favorites` : `Add ${tool.name} to favorites`} aria-pressed={box.isFavorite(tool.id)} title={box.isFavorite(tool.id) ? 'Remove from favorites' : 'Add to favorites'} onclick={(event) => void box.toggleFavorite(tool, event)}><Icon name="star" size={14} /></button>
        </div>
      {/each}
    {/if}
  </div>
{:else if box.focused && box.runnableContextSuggestions.length > 0}
  <div class="overlay-body context-panel" id="context-list" role="listbox" aria-label="Suggestions for the clipboard">
    <div class="t-caption group-label">From your clipboard</div>
    {#each box.runnableContextSuggestions as suggestion, index}
      {@const suggestedTool = box.toolById.get(suggestion.toolId)}
      {#if suggestedTool}
        <button type="button" id={`context-${suggestion.toolId}`} role="option" class="overlay-row" aria-selected={index === box.selectedIndex} onclick={() => box.selectContextSuggestion(suggestion)} onmouseenter={() => (box.selectedIndex = index)}>
          <span class="glyph-tile sm"><Icon name={box.iconForCategory(suggestedTool.category)} size={14} /></span>
          <span class="row-main"><span class="row-title ellipsis">{suggestedTool.name}</span></span>
          <span class="row-sub ellipsis suggestion-reason">{suggestion.reason}</span>
          {#if index === box.selectedIndex}<kbd class="key-chip" aria-hidden="true">↵</kbd>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

{#if box.catalogError && !box.query.trim()}
  <div class="notice warn island-notice" role="status"><Icon name="alert" /><span class="row-main">{box.catalogError}</span></div>
{/if}

<style>
  .island-search { gap: var(--sp-10); border-bottom: 0; }
  .island-search.has-query, :global(.island-shell.expanded) .island-search { border-bottom: 1px solid var(--line); }
  .island-mark { width: 24px; height: 24px; }
  .shortcut-hint { flex: none; }
  .search-panel, .context-panel { max-height: min(56dvh, 452px); padding: var(--sp-4) var(--sp-6) var(--sp-6); overscroll-behavior: contain; }
  .group-label { padding: var(--sp-10) var(--sp-10) var(--sp-4); color: var(--faint); }
  .result-row { position: relative; min-width: 0; }
  .overlay-row { width: 100%; border: 0; background: transparent; color: inherit; font: inherit; text-align: left; }
  .overlay-row .row-main { display: grid; min-width: 0; }
  .overlay-row .row-title { min-width: 0; }
  .result-row .overlay-row { padding-right: 76px; }
  .result-row .key-chip { position: absolute; right: 44px; top: 50%; transform: translateY(-50%); }
  .favorite { position: absolute; top: 50%; right: var(--sp-8); transform: translateY(-50%); opacity: 0; }
  .result-row:hover .favorite, .favorite:focus-visible, .favorite.on { opacity: 1; }
  .favorite.on { color: var(--accent); }
  .suggestion-reason { flex: 0 1 45%; margin: 0; text-align: right; }
  .island-empty { display: grid; justify-items: center; gap: var(--sp-4); padding: var(--sp-24) var(--sp-16); color: var(--muted); text-align: center; }
  .island-empty strong { color: var(--text); font-weight: var(--fw-medium); }
  .island-notice { margin: var(--sp-8) var(--sp-10) var(--sp-10); }
  .link-pending { display: flex; align-items: center; gap: var(--sp-8); margin: var(--sp-10) var(--sp-12) 0; padding: var(--sp-4) var(--sp-4) var(--sp-4) var(--sp-12); border-radius: var(--radius-sm); background: var(--accent-soft); color: var(--text); font-size: var(--fs-small); }
  .link-pending span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
