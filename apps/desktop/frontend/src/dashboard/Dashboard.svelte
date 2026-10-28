<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import PipelinesView from '../lib/PipelinesView.svelte';
  import PluginManager from '../lib/PluginManager.svelte';
  import SettingsView from '../lib/SettingsView.svelte';
  import { isRunnable, privacyLabel } from '../lib/tool-utils';
  import { useBox } from '../box.svelte';

  const box = useBox();
</script>
<section class="dashboard-shell" aria-label="Arcade Box dashboard">
  <aside class="sidebar">
    <div class="sidebar-brand"><span class="brand-mark"><Icon name="command" size={20} /></span><span class="brand-name">Arcade <span>Box</span></span></div>
    <div class="sidebar-section-label">Workspace</div>
    <nav class="main-nav" aria-label="Workspace">
      <button class:nav-active={box.catalogMode === 'all' && box.categoryFilter === 'All tools'} aria-current={box.catalogMode === 'all' && box.categoryFilter === 'All tools' ? 'page' : undefined} onclick={() => { box.catalogMode = 'all'; box.categoryFilter = 'All tools'; }}><Icon name="grid" size={17} /><span>Tools</span><span class="nav-count">{box.tools.length}</span></button>
      <button class:nav-active={box.catalogMode === 'favorites'} aria-current={box.catalogMode === 'favorites' ? 'page' : undefined} onclick={() => { box.catalogMode = 'favorites'; box.categoryFilter = 'All tools'; }}><Icon name="star" size={17} /><span>Favorites</span>{#if box.favorites.length}<span class="nav-count">{box.favorites.length}</span>{/if}</button>
      <button class:nav-active={box.catalogMode === 'recent'} aria-current={box.catalogMode === 'recent' ? 'page' : undefined} onclick={() => { box.catalogMode = 'recent'; box.categoryFilter = 'All tools'; }}><Icon name="clock" size={17} /><span>Recent</span>{#if box.recentToolIds.length}<span class="nav-count">{box.recentToolIds.length}</span>{/if}</button>
      <button class:nav-active={box.catalogMode === 'jobs'} aria-current={box.catalogMode === 'jobs' ? 'page' : undefined} onclick={() => { box.catalogMode = 'jobs'; box.categoryFilter = 'All tools'; }}><Icon name="play" size={17} /><span>Jobs</span>{#if box.activeJobCount}<span class="nav-count">{box.activeJobCount}</span>{/if}</button>
      <button class:nav-active={box.catalogMode === 'pipelines'} aria-current={box.catalogMode === 'pipelines' ? 'page' : undefined} onclick={() => { box.catalogMode = 'pipelines'; box.categoryFilter = 'All tools'; }}><Icon name="spark" size={17} /><span>Pipelines</span></button>
      <button class:nav-active={box.catalogMode === 'plugins'} aria-current={box.catalogMode === 'plugins' ? 'page' : undefined} onclick={() => { box.catalogMode = 'plugins'; box.categoryFilter = 'All tools'; }}><Icon name="shield" size={17} /><span>Plugins</span></button>
      <button class:nav-active={box.catalogMode === 'engines'} aria-current={box.catalogMode === 'engines' ? 'page' : undefined} onclick={() => { box.catalogMode = 'engines'; box.categoryFilter = 'All tools'; }}><Icon name="network" size={17} /><span>Engines</span><span class="nav-count">{box.providers.length}</span></button>
      <button class:nav-active={box.catalogMode === 'settings'} aria-current={box.catalogMode === 'settings' ? 'page' : undefined} onclick={() => { box.catalogMode = 'settings'; box.categoryFilter = 'All tools'; }}><Icon name="settings" size={17} /><span>Settings</span></button>
    </nav>
    <div class="sidebar-section-row"><span class="sidebar-section-label">Categories</span><button aria-label="Show all categories" onclick={() => { box.catalogMode = 'all'; box.categoryFilter = 'All tools'; }}><Icon name="plus" size={14} /></button></div>
    <nav class="category-nav" aria-label="Tool categories">
      {#each box.categories as category}
        <button class:nav-active={box.categoryFilter === category} aria-current={box.categoryFilter === category ? 'page' : undefined} onclick={() => box.chooseCategory(category)}><Icon name={box.iconForCategory(category)} size={16} /><span>{category}</span></button>
      {/each}
      {#if box.categories.length === 0 && !box.loadingCatalog}
        <span class="sidebar-empty">Categories will appear when the local catalog connects.</span>
      {/if}
    </nav>
    <div class="sidebar-bottom">
      <div class="profile-button"><span class="profile-avatar">A</span><span><strong>Arcade Box</strong><small>Desktop workspace</small></span><Icon name="dots" size={17} /></div>
    </div>
  </aside>

  <div class="dashboard-main">
    <div class="sr-only" role="status" aria-live="polite" aria-atomic="true">{box.dashboardSearchAnnouncement}</div>
    <header class="dashboard-topbar">
      <div class="breadcrumbs"><span>Arcade Box</span><Icon name="chevron" size={14} /><strong>{box.catalogMode === 'recent' ? 'Recently used' : box.catalogMode === 'favorites' ? 'Favorites' : box.catalogMode === 'jobs' ? 'Background jobs' : box.catalogMode === 'engines' ? 'Engines & dependencies' : box.catalogMode === 'pipelines' ? 'Pipelines' : box.catalogMode === 'plugins' ? 'Plugins' : box.catalogMode === 'settings' ? 'Settings' : box.categoryFilter}</strong></div>
      <div class="topbar-actions"><span class="sync-state"><span class="runtime-dot" class:offline={Boolean(box.catalogError)} aria-hidden="true"></span>{box.catalogError ? 'Runtime offline' : 'Local runtime'}</span><button bind:this={box.dashboardCloseButton} class="quiet-button" aria-label="Close Arcade Box dashboard" onclick={box.toggleDashboard}><Icon name="close" size={16} /><span>Close</span></button></div>
    </header>

    <div class="dashboard-content">
      <section class="dashboard-intro">
        <div class="intro-copy"><h1 aria-live="polite" aria-atomic="true">{box.catalogMode === 'recent' ? 'Recently used' : box.catalogMode === 'favorites' ? 'Favorites' : box.catalogMode === 'jobs' ? 'Background jobs' : box.catalogMode === 'engines' ? 'Engines' : box.catalogMode === 'pipelines' ? 'Pipelines' : box.catalogMode === 'plugins' ? 'Plugins' : box.catalogMode === 'settings' ? 'Settings' : box.categoryFilter === 'All tools' ? 'Tools' : box.categoryFilter}</h1><p>{box.catalogMode === 'recent' ? 'Your recent actions are saved on this device.' : box.catalogMode === 'favorites' ? 'The tools you want close at hand.' : box.catalogMode === 'jobs' ? 'Long-running work continues while the Island is closed.' : box.catalogMode === 'engines' ? 'Providers detected by the local runtime.' : box.catalogMode === 'pipelines' ? 'Save compatible actions as one reusable workflow.' : box.catalogMode === 'plugins' ? 'Review permissions before installing community tools.' : box.catalogMode === 'settings' ? 'Appearance, shortcuts, and privacy controls for this device.' : 'Find a tool. Get it done. Get back to your day.'}</p></div>
      </section>

      {#if box.catalogMode !== 'engines' && box.catalogMode !== 'jobs' && box.catalogMode !== 'pipelines' && box.catalogMode !== 'plugins' && box.catalogMode !== 'settings'}
        <div class="dashboard-toolbar">
          <div class="catalog-search"><Icon name="search" size={18} /><input bind:this={box.catalogSearchInput} bind:value={box.dashboardQuery} aria-label="Filter tool catalog" placeholder="Search the catalog…" /><kbd>{box.commandKey} K</kbd></div>
          <div class="catalog-summary"><span class="summary-dot"></span>{box.implementedTools.length} ready{#if box.partialTools.length}<span class="summary-divider"></span>{box.partialTools.length} in progress{/if}<span class="summary-divider"></span>{box.plannedTools.length} planned</div>
        </div>
      {/if}

      {#if box.catalogMode === 'pipelines'}
        <PipelinesView tools={box.tools} onToolsChanged={box.acceptToolCatalog} />
      {:else if box.catalogMode === 'plugins'}
        <PluginManager onToolsChanged={box.acceptToolCatalog} />
      {:else if box.catalogMode === 'settings'}
        <SettingsView theme={box.themeMode} shortcut={box.shortcutInfo} onThemeChange={box.changeTheme} onShortcutChange={box.acceptShortcutStatus} />
      {:else if box.catalogMode === 'jobs'}
        {#if box.jobError}<div class="dashboard-alert" role="alert"><span class="alert-icon"><Icon name="network" size={16} /></span><div><strong>Could not update job</strong><span>{box.jobError}</span></div></div>{/if}
        {#if box.backgroundJobs.length === 0}
          <div class="provider-empty"><span class="provider-empty-icon"><Icon name="clock" size={20} /></span><div><strong>No background jobs yet</strong><span>Video conversions and other long operations will appear here.</span></div></div>
        {:else}
          <section class="catalog-section provider-section">
            <div class="catalog-section-heading"><div><h2>Recent jobs</h2><span>Jobs remain available after closing Arcade Island</span></div><span class="section-count">{box.backgroundJobs.length}</span></div>
            <div class="job-list">
              {#each box.backgroundJobs as job (job.id)}
                {@const jobTool = box.tools.find((tool) => tool.id === job.toolId)}
                <article class="job-card">
                  <span class="job-icon"><Icon name={jobTool ? box.iconForCategory(jobTool.category) : 'spark'} size={17} /></span>
                  <div class="job-copy"><div class="job-title-row"><strong>{jobTool?.name || job.toolId}</strong><span class="job-status" class:job-status-done={job.status === 'succeeded'} class:job-status-error={job.status === 'failed' || job.status === 'interrupted' || job.status === 'cancelled'}>{box.jobStatusText(job.status)}</span></div><span>{job.message || (box.jobIsActive(job) ? 'Working in the background' : 'Job finished')}</span>{#if box.jobIsActive(job)}<div class="job-progress-track" role="progressbar" aria-label="{jobTool?.name || 'Background operation'} progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={job.progress === null ? undefined : Math.round(Math.max(0, Math.min(1, job.progress)) * 100)} aria-valuetext={job.progress === null ? 'Progress is not available' : `${Math.round(Math.max(0, Math.min(1, job.progress)) * 100)} percent`}><span style={`width:${Math.max(0, Math.min(100, (job.progress ?? 0) * 100))}%`}></span></div>{/if}</div>
                  <div class="job-actions">
                    {#if box.jobIsActive(job)}<button class="quiet-button cancel-job-button" disabled={box.cancellingJobId === job.id || job.status === 'cancelling'} onclick={() => void box.cancelBackgroundJob(job)}><Icon name="close" size={14} /><span>{box.cancellingJobId === job.id || job.status === 'cancelling' ? 'Stopping' : 'Cancel'}</span></button>{/if}
                    {#if job.result && jobTool}<button class="quiet-button" onclick={() => box.viewJobResult(job)}><Icon name="external" size={14} /><span>View result</span></button>{/if}
                  </div>
                </article>
              {/each}
            </div>
          </section>
        {/if}
      {:else if box.catalogMode === 'engines'}
        {#if box.providerError}
          <div class="dashboard-alert" role="alert"><span class="alert-icon"><Icon name="network" size={16} /></span><div><strong>Could not inspect providers</strong><span>{box.providerError}</span></div></div>
        {/if}
        {#if box.tesseractMissing}
          <div class="provider-inline-missing" role="status"><Icon name="document" size={14} /><span>{box.tesseractMessage || (box.tesseractOffer ? 'Tesseract (text recognition) isn’t installed. Arcade apps can download a private copy shared by all of them.' : 'Tesseract (text recognition) isn’t installed. Install it with its English language data from your package manager.')}</span>{#if box.tesseractOffer}<button type="button" disabled={box.tesseractBusy} onclick={() => void box.getTesseract()}>{box.tesseractBusy ? 'Downloading…' : `Download Tesseract (${box.tesseractOffer} MB)`}</button>{/if}</div>
        {/if}
        {#if box.providers.length === 0}
          <div class="provider-empty"><span class="provider-empty-icon"><Icon name="network" size={20} /></span><div><strong>No providers detected</strong><span>Arcade Box checks for supported system tools such as FFmpeg, qpdf, and libvips when the runtime starts.</span></div></div>
        {:else}
          <section class="catalog-section provider-section">
            <div class="catalog-section-heading"><div><h2>Detected providers</h2><span>System installations are shown with their verified paths</span></div><span class="section-count">{box.providers.length}</span></div>
            <div class="provider-list">
              {#each box.providers as provider (`${provider.capability}-${provider.executablePath}`)}
                <article class="provider-card">
                  <span class="provider-icon"><Icon name={box.providerIcon(provider.capability)} size={19} /></span>
                  <div class="provider-copy"><div class="provider-title-row"><strong>{provider.capability === 'ocr.lens' ? 'Arcade Lens (local)' : provider.capability === 'screen.select.lens' ? 'Arcade Lens screen selection' : provider.capability}</strong><span class="provider-source">{provider.source === 'system' ? 'System installation' : provider.source === 'arcade-app' ? 'Local Arcade app' : provider.source}</span></div><span class="provider-version">Version {provider.version}</span><code>{provider.executablePath}</code>{#if provider.warning}<span class="provider-warning">{provider.warning}</span>{/if}{#if provider.capabilities?.length}<details class="provider-capabilities"><summary>Capabilities · {provider.capabilities.length}</summary><div>{#each provider.capabilities as capability}<span>{capability}</span>{/each}</div></details>{/if}</div>
                  <span class="provider-status" class:provider-incompatible={!provider.compatible}><span></span>{provider.compatible ? 'Compatible' : 'Needs attention'}</span>
                </article>
              {/each}
            </div>
          </section>
        {/if}
      {:else if box.catalogError}
        <div class="dashboard-alert" role="alert"><span class="alert-icon"><Icon name="network" size={16} /></span><div><strong>Local tool runtime unavailable</strong><span>{box.catalogError}</span></div></div>
      {/if}

      {#if box.catalogMode === 'settings'}
        <!-- Settings are rendered from persisted preferences above. -->
      {:else if box.catalogMode === 'engines' || box.catalogMode === 'jobs' || box.catalogMode === 'pipelines' || box.catalogMode === 'plugins'}
        <!-- Specialized views is rendered above from live provider discovery. -->
      {:else if box.loadingCatalog}
        <div class="catalog-loading"><span class="spinner"></span><span>Connecting to the local catalog…</span></div>
      {:else if box.visibleDashboardTools.length === 0}
        <div class="catalog-empty">
          <div class="empty-icon"><Icon name={box.catalogMode === 'recent' ? 'clock' : box.catalogMode === 'favorites' ? 'star' : 'search'} size={21} /></div>
          <strong>{box.catalogMode === 'recent' ? 'Nothing here yet' : box.catalogMode === 'favorites' ? 'No favorites yet' : box.catalogError ? 'Catalog is waiting for the desktop core' : 'No tools match this view'}</strong>
          <span>{box.catalogMode === 'recent' ? 'Run an action from the Island and it will appear here.' : box.catalogMode === 'favorites' ? 'Use the star beside a tool to keep it close.' : box.catalogError ? 'The frontend connects to the Rust tool registry when Arcade Box starts as a desktop app.' : 'Try another search or choose a different category.'}</span>
          {#if box.catalogMode === 'recent'}<button class="text-action" onclick={box.toggleDashboard}>Go to all tools <Icon name="arrow" size={14} /></button>{/if}
        </div>
      {:else}
        {@const dashboardReady = box.visibleDashboardTools.filter(isRunnable)}
        {@const dashboardPlanned = box.visibleDashboardTools.filter((tool) => !isRunnable(tool))}
        {#if dashboardReady.length > 0}
          <section class="catalog-section">
            <div class="catalog-section-heading"><div><h2>{box.catalogMode === 'recent' ? 'Recent actions' : box.catalogMode === 'favorites' ? 'Favorite actions' : 'Tools'}</h2><span>Some tools require an installed engine. Open a tool for details.</span></div><span class="section-count">{dashboardReady.length}</span></div>
            <div class="tool-grid">
              {#each dashboardReady as tool (tool.id)}
                <div class="catalog-card-wrap">
                  <button class="catalog-card" onclick={() => box.openTool(tool, true)}>
                    <span class="catalog-card-top"><span class="catalog-icon"><Icon name={box.iconForCategory(tool.category)} size={19} /></span><span class="privacy-tag" class:network={privacyLabel(tool.privacyClass) === 'NETWORK'} class:cloud={privacyLabel(tool.privacyClass) === 'CLOUD'}>{privacyLabel(tool.privacyClass)}</span></span>
                    <span class="catalog-card-title">{tool.name}</span><span class="catalog-card-description">{tool.description}</span>
                    <span class="catalog-card-bottom"><span>{tool.category}</span>{#if tool.status === 'partial'}<span class="availability-label">In progress</span>{/if}<span class="card-arrow"><Icon name="arrow" size={14} /></span></span>
                  </button>
                  <button class="favorite-toggle card-favorite" class:favorite-on={box.isFavorite(tool.id)} aria-label={box.isFavorite(tool.id) ? `Remove ${tool.name} from favorites` : `Add ${tool.name} to favorites`} aria-pressed={box.isFavorite(tool.id)} title={box.isFavorite(tool.id) ? 'Remove from favorites' : 'Add to favorites'} onclick={(event) => void box.toggleFavorite(tool, event)}><Icon name="star" size={15} /></button>
                </div>
              {/each}
            </div>
          </section>
        {/if}
        {#if dashboardPlanned.length > 0}
          <section class="catalog-section planned-section">
            <div class="catalog-section-heading"><div><h2>Coming to Arcade Box</h2><span>Visible for discovery, unavailable to run</span></div><span class="section-count muted-count">{dashboardPlanned.length}</span></div>
            <div class="planned-grid">
              {#each dashboardPlanned as tool (tool.id)}
                <button class="planned-card" onclick={() => box.openTool(tool, true)}>
                  <span class="planned-card-icon"><Icon name={box.iconForCategory(tool.category)} size={17} /></span><span class="planned-card-copy"><strong>{tool.name}</strong><small>{tool.category} · {privacyLabel(tool.privacyClass)}</small></span><span class="planned-pill">Planned</span>
                </button>
              {/each}
            </div>
          </section>
        {/if}
      {/if}

      {#if box.favoriteError}<div class="field-error" role="alert">{box.favoriteError}</div>{/if}
      <footer class="dashboard-footer"><span><span class="footer-orb"></span> Arcade Box runs on your device</span></footer>
    </div>
  </div>
</section>
