<script lang="ts">
  import { arcadeAppName } from '../lib/arcade';
  import Icon from '../lib/Icon.svelte';
  import { privacyLabel } from '../lib/tool-utils';
  import { useBox } from '../box.svelte';

  const box = useBox();
</script>
{#if box.pendingLink}
  <div class="link-pending" role="status">
    <span>From {arcadeAppName(box.pendingLink.source)}: {box.pendingLink.files.length ? `${box.pendingLink.files.length === 1 ? box.pendingLink.files[0].name : `${box.pendingLink.files.length} files`}` : 'text'}. Choose a tool to use it.</span>
    <button class="clear-search" aria-label="Discard the input from the other app" onclick={() => (box.pendingLink = null)}><Icon name="close" size={14} /></button>
  </div>
{/if}
<div class="search-row" class:has-query={box.query.trim().length > 0}>
  <span class="search-leading"><Icon name="search" size={20} /></span>
  <input
    bind:this={box.searchInput}
    bind:value={box.query}
    onfocus={() => (box.focused = true)}
    onblur={(event) => {
      const nextFocus = event.relatedTarget;
      box.focused = nextFocus instanceof Node && Boolean(box.islandShell?.contains(nextFocus));
    }}
    onkeydown={box.handleSearchKeydown}
    aria-label="Search tools and actions"
    aria-describedby="search-keyboard-help"
    aria-keyshortcuts="ArrowDown ArrowUp Enter Escape"
    autocomplete="off"
    autocapitalize="off"
    spellcheck="false"
    placeholder="What do you need done?"
  />
  {#if box.query}
    <button class="clear-search" aria-label="Clear search" onclick={() => { box.query = ''; box.searchInput?.focus(); }}><Icon name="close" size={16} /></button>
  {:else}
    <button class="icon-button search-browse" aria-label="Browse all tools" title="Browse all tools" onclick={box.toggleDashboard}><Icon name="grid" size={17} /></button>
  {/if}
</div>

{#if box.query.trim()}
  <div class="search-panel" id="search-list" role="region" aria-label="Tool search results">
    {#if box.searchError}
      <div class="search-state error-state"><span class="state-symbol"><Icon name="network" size={18} /></span><div><strong>Local runtime unavailable</strong><span>{box.searchError}</span></div></div>
    {:else if box.searchResults.length === 0 && box.loadingSearch}
      <div class="search-state search-pending"><span class="spinner"></span><span>Finding the right action…</span></div>
    {:else if box.searchResults.length === 0}
      <div class="search-state empty-state"><span class="state-symbol"><Icon name="search" size={17} /></span><div><strong>No matching tools</strong><span>Try another phrase or browse the catalog.</span></div></div>
    {:else}
      {#if box.runnableSearchResults.length > 0}
        <div class="result-group-label"><span>Actions</span><span>{box.runnableSearchResults.length}</span></div>
        <div class="search-results">
          {#each box.runnableSearchResults as tool, index (tool.id)}
            <div class="search-result-row">
              <button
                id={`tool-${tool.id}`}
                class="search-result"
                class:active={index === box.selectedIndex}
                onclick={() => box.openTool(tool)}
                onmouseenter={() => (box.selectedIndex = index)}
              >
                <span class="result-tool-icon"><Icon name={box.iconForCategory(tool.category)} size={18} /></span>
                <span class="result-tool-copy"><strong>{tool.name}</strong><span>{tool.description}</span></span>
                <span class="result-meta">{#if tool.status === 'partial'}<span class="availability-label">In progress</span>{/if}<span class="privacy-mini" class:network={privacyLabel(tool.privacyClass) === 'NETWORK'} class:cloud={privacyLabel(tool.privacyClass) === 'CLOUD'}>{privacyLabel(tool.privacyClass)}</span><Icon name="chevron" size={15} /></span>
              </button>
              <button class="favorite-toggle search-favorite" class:favorite-on={box.isFavorite(tool.id)} aria-label={box.isFavorite(tool.id) ? `Remove ${tool.name} from favorites` : `Add ${tool.name} to favorites`} aria-pressed={box.isFavorite(tool.id)} title={box.isFavorite(tool.id) ? 'Remove from favorites' : 'Add to favorites'} onclick={(event) => void box.toggleFavorite(tool, event)}><Icon name="star" size={15} /></button>
            </div>
          {/each}
        </div>
      {/if}
      {#if box.plannedSearchResults.length > 0}
        <div class="result-group-label discovery-label"><span>Planned</span><span>{box.plannedSearchResults.length}</span></div>
        <div class="search-results planned-results">
          {#each box.plannedSearchResults as tool (tool.id)}
            <div class="search-result-row">
              <button class="search-result planned-result" onclick={() => box.openTool(tool)}>
                <span class="result-tool-icon planned-icon"><Icon name={box.iconForCategory(tool.category)} size={18} /></span>
                <span class="result-tool-copy"><strong>{tool.name}</strong><span>{tool.description}</span></span>
                <span class="planned-label">Planned</span>
              </button>
              <button class="favorite-toggle search-favorite" class:favorite-on={box.isFavorite(tool.id)} aria-label={box.isFavorite(tool.id) ? `Remove ${tool.name} from favorites` : `Add ${tool.name} to favorites`} aria-pressed={box.isFavorite(tool.id)} title={box.isFavorite(tool.id) ? 'Remove from favorites' : 'Add to favorites'} onclick={(event) => void box.toggleFavorite(tool, event)}><Icon name="star" size={15} /></button>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
{:else if box.focused}
  {#if box.runnableContextSuggestions.length > 0}
    <div class="context-panel" id="context-list" role="region" aria-label="Actions for the current clipboard">
      <div class="context-label"><Icon name="spark" size={14} /><span>From your clipboard</span><span class="local-chip">Local</span></div>
      {#each box.runnableContextSuggestions as suggestion, index}
        {@const suggestedTool = box.tools.find((tool) => tool.id === suggestion.toolId)}
        {#if suggestedTool}
          <button id={`context-${suggestion.toolId}`} class="context-suggestion" class:active={index === box.selectedIndex} onclick={() => box.selectContextSuggestion(suggestion)} onmouseenter={() => (box.selectedIndex = index)}>
            <span class="result-tool-icon"><Icon name={box.iconForCategory(suggestedTool.category)} size={17} /></span>
            <span class="result-tool-copy"><strong>{suggestedTool.name}</strong><span>{suggestion.reason}</span></span>
            <Icon name="chevron" size={15} />
          </button>
        {/if}
      {/each}
    </div>
  {:else}
    <div class="welcome-prompt">
      <div class="welcome-orbit"><Icon name="spark" size={21} /></div>
      <div><strong>Ready when you are.</strong><span>Search for a task, or browse your tools.</span></div>
    </div>
  {/if}
{/if}

{#if box.catalogError && !box.query.trim()}
  <div class="runtime-notice" role="status"><span class="runtime-notice-dot"></span><span>{box.catalogError}</span></div>
{/if}
