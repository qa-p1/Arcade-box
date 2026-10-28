<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import type { IconName } from '../lib/icon-names';
  import boxGlyph from '../design/glyphs/arcade.box.svg';
  import { useBox } from '../box.svelte';

  const box = useBox();
  type Page = 'all' | 'favorites' | 'recent' | 'jobs' | 'pipelines' | 'plugins' | 'engines' | 'settings';
  const pages: { id: Page; label: string; icon: IconName }[] = [
    { id: 'all', label: 'Tools', icon: 'apps' },
    { id: 'favorites', label: 'Favorites', icon: 'star' },
    { id: 'recent', label: 'Recent', icon: 'clock' },
    { id: 'jobs', label: 'Jobs', icon: 'play' },
    { id: 'pipelines', label: 'Pipelines', icon: 'sparkle' },
    { id: 'plugins', label: 'Plugins', icon: 'shield' },
    { id: 'engines', label: 'Engines', icon: 'engine' },
    { id: 'settings', label: 'Settings', icon: 'settings' },
  ];
  const counts = $derived<Partial<Record<Page, number>>>({
    all: box.runnableTools.length,
    favorites: box.favorites.length,
    recent: box.recentToolIds.length,
    jobs: box.activeJobCount,
  });
  const onPage = (page: Page) => box.catalogMode === page && (page !== 'all' || box.categoryFilter === 'All tools');

  function choose(page: Page): void {
    box.catalogMode = page;
    box.categoryFilter = 'All tools';
  }
</script>

<aside class="sidebar dashboard-sidebar" aria-label="Arcade Box">
  <div class="brand"><span class="brand-mark" aria-hidden="true"><img src={boxGlyph} alt="" width="16" height="16" /></span><span>Arcade Box</span></div>
  <nav class="tab-list" aria-label="Dashboard">
    {#each pages as page (page.id)}
      <button
        type="button"
        class="tab-button"
        class:active={onPage(page.id)}
        aria-current={onPage(page.id) ? 'page' : undefined}
        aria-label={counts[page.id] ? `${page.label}, ${counts[page.id]}` : page.label}
        onclick={() => choose(page.id)}
      ><Icon name={page.icon} /><span class="label">{page.label}</span>{#if counts[page.id]}<span class="count" aria-hidden="true">{counts[page.id]}</span>{/if}</button>
    {/each}
  </nav>
  <div class="t-caption nav-caption" id="categories-label">Categories</div>
  <nav class="tab-list category-list" aria-labelledby="categories-label">
    {#each box.categories as category (category)}
      <button
        type="button"
        class="tab-button"
        class:active={box.catalogMode === 'all' && box.categoryFilter === category}
        aria-current={box.catalogMode === 'all' && box.categoryFilter === category ? 'page' : undefined}
        onclick={() => box.chooseCategory(category)}
      ><Icon name={box.iconForCategory(category)} /><span class="label">{category}</span></button>
    {/each}
    {#if box.categories.length === 0 && !box.loadingCatalog}
      <p class="sidebar-empty">Categories appear once the tools load.</p>
    {/if}
  </nav>
  <div class="sidebar-foot"><div class="sidebar-meta"><span>Version {box.version}</span></div></div>
</aside>

<style>
  .dashboard-sidebar { overflow: hidden; }
  /* minmax(0, 1fr): an auto track grows to the longest label (the 0.3 bug). */
  .tab-list { grid-template-columns: minmax(0, 1fr); }
  .nav-caption { margin: var(--sp-20) var(--sp-10) var(--sp-6); color: var(--faint); }
  /* The categories scroll on their own, so a short window keeps the pages,
     the footer and every label (ellipsised) in view. */
  .category-list { flex: 1 1 auto; min-height: 0; overflow-x: hidden; overflow-y: auto; overscroll-behavior: contain; align-content: start; }
  .sidebar-empty { padding: var(--sp-6) var(--sp-10); color: var(--faint); font-size: var(--fs-small); }
  .sidebar-foot { flex: none; margin-top: var(--sp-12); }
</style>
