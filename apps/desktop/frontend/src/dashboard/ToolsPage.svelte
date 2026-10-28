<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import { openIsland } from '../lib/arcade';
  import { privacyLabel } from '../lib/tool-utils';
  import { useBox } from '../box.svelte';

  const box = useBox();
  const category = $derived(box.categoryFilter === 'All tools' ? null : box.categoryFilter);
  const title = $derived(box.catalogMode === 'favorites' ? 'Favorites' : box.catalogMode === 'recent' ? 'Recent' : category ?? 'Tools');
  const count = $derived(box.visibleDashboardTools.length);
  const subtitle = $derived(
    box.catalogMode === 'recent' ? 'Tools you used lately, newest first.'
      : box.dashboardQuery.trim() ? `${count} of ${box.runnableTools.length} tools`
        : `${count} ${count === 1 ? 'tool' : 'tools'}`,
  );
</script>

<header class="page-head">
  <div>
    <h1>{title}</h1>
    <p class="sub">{subtitle}</p>
  </div>
  <div class="page-actions">
    <button type="button" class="btn primary" onclick={() => void openIsland()}><Icon name="search" size={14} />Open Island</button>
  </div>
</header>

<div class="tools-toolbar">
  <label class="search-field">
    <Icon name="search" size={16} />
    <input bind:this={box.catalogSearchInput} bind:value={box.dashboardQuery} type="search" aria-label="Search tools" placeholder="Search tools" autocomplete="off" spellcheck="false" />
    {#if box.dashboardQuery}<button type="button" class="search-clear" aria-label="Clear search" onclick={() => { box.dashboardQuery = ''; box.catalogSearchInput?.focus(); }}><Icon name="close" size={14} /></button>{:else}<kbd class="key-chip">{box.commandKey} K</kbd>{/if}
  </label>
  {#if box.catalogMode === 'all'}
    <select class="input select" aria-label="Category" value={box.categoryFilter} onchange={(event) => { const value = event.currentTarget.value; if (value === 'All tools') box.categoryFilter = value; else box.chooseCategory(value); }}>
      <option value="All tools">All categories</option>
      {#each box.categories as name (name)}<option value={name}>{name}</option>{/each}
    </select>
  {/if}
</div>
<div class="visually-hidden" role="status" aria-live="polite" aria-atomic="true">{box.dashboardSearchAnnouncement}</div>

{#if box.catalogError}
  <div class="notice danger" role="alert"><Icon name="alert" /><span class="row-main">Couldn't load the tools. {box.catalogError}</span></div>
{:else if box.loadingCatalog}
  <div class="card-grid" aria-busy="true" aria-label="Loading tools">
    {#each [0, 1, 2, 3, 4, 5] as item (item)}<div class="card tool-card skeleton-card"><div class="skeleton tile"></div><div class="skeleton line"></div><div class="skeleton line short"></div></div>{/each}
  </div>
{:else if count === 0}
  <div class="empty-state">
    <Icon name={box.catalogMode === 'recent' ? 'clock' : box.catalogMode === 'favorites' ? 'star' : 'search'} size={24} />
    {#if box.dashboardQuery.trim()}
      <h2>No tools match “{box.dashboardQuery.trim()}”</h2><p>Try another word, or clear the search.</p>
      <div class="btn-group"><button type="button" class="btn" onclick={() => { box.dashboardQuery = ''; }}>Clear search</button></div>
    {:else if box.catalogMode === 'favorites'}
      <h2>No favorites yet</h2><p>Star a tool to keep it here.</p>
    {:else if box.catalogMode === 'recent'}
      <h2>Nothing used yet</h2><p>Tools you run from the Island show up here.</p>
      <div class="btn-group"><button type="button" class="btn" onclick={() => { box.catalogMode = 'all'; box.categoryFilter = 'All tools'; }}>Browse tools</button></div>
    {:else}
      <h2>No tools here</h2><p>This category has no tools yet.</p>
    {/if}
  </div>
{:else}
  <div class="card-grid">
    {#each box.visibleDashboardTools as tool (tool.id)}
      {@const online = privacyLabel(tool.privacyClass) !== 'LOCAL'}
      <div class="tool-card-wrap">
        <button type="button" class="card clickable tool-card" onclick={() => box.openTool(tool, true)}>
          <span class="glyph-tile"><Icon name={box.iconForCategory(tool.category)} size={18} /></span>
          <span class="tool-name">{tool.name}</span>
          <span class="tool-description">{tool.description}</span>
          {#if online}<span class="badge warn tool-badge" title="Uses an online service">online</span>{/if}
        </button>
        <button
          type="button"
          class="btn ghost icon-only sm favorite"
          class:on={box.isFavorite(tool.id)}
          aria-label={box.isFavorite(tool.id) ? `Remove ${tool.name} from favorites` : `Add ${tool.name} to favorites`}
          aria-pressed={box.isFavorite(tool.id)}
          title={box.isFavorite(tool.id) ? 'Remove from favorites' : 'Add to favorites'}
          onclick={(event) => void box.toggleFavorite(tool, event)}
        ><Icon name="star" size={14} /></button>
      </div>
    {/each}
  </div>
{/if}
{#if box.favoriteError}<p class="error-text" role="alert">{box.favoriteError}</p>{/if}

<style>
  .tools-toolbar { display: flex; flex-wrap: wrap; gap: var(--sp-8); margin-bottom: var(--sp-16); }
  .tools-toolbar .search-field { flex: 1 1 260px; min-width: 0; }
  .tools-toolbar .select { flex: 0 1 220px; min-width: 0; height: var(--h-search); }
  .tool-card-wrap { position: relative; min-width: 0; }
  .tool-card { display: grid; grid-template-columns: auto minmax(0, 1fr); grid-template-rows: auto auto 1fr; column-gap: var(--sp-12); row-gap: var(--sp-2); width: 100%; height: 100%; min-width: 0; padding-right: 44px; color: inherit; font: inherit; text-align: left; }
  .tool-card .glyph-tile { grid-row: 1 / 3; }
  .tool-name { min-width: 0; font-weight: var(--fw-medium); overflow-wrap: anywhere; }
  .tool-description { display: -webkit-box; min-width: 0; overflow: hidden; color: var(--muted); font-size: var(--fs-small); line-height: var(--lh-small); -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; }
  .tool-badge { grid-column: 2; justify-self: start; margin-top: var(--sp-6); }
  .favorite { position: absolute; top: var(--sp-10); right: var(--sp-10); opacity: 0; }
  .tool-card-wrap:hover .favorite, .favorite:focus-visible, .favorite.on { opacity: 1; }
  .favorite.on { color: var(--accent); }
  .skeleton-card { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--sp-8) var(--sp-12); }
  .skeleton.tile { grid-row: 1 / 3; width: var(--tile); height: var(--tile); border-radius: var(--radius-tile); }
  .skeleton.short { width: 60%; }
  .error-text { margin-top: var(--sp-10); }
</style>
