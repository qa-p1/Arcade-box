<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import type { PluginPermissionGrant, PluginPreview, PluginSummary, ToolSummary } from './contracts';
  import { choosePluginPackage, installPlugin, listPlugins, listTools, previewPlugin, uninstallPlugin } from './arcade';

  let {
    onToolsChanged,
  }: {
    onToolsChanged: (tools: ToolSummary[]) => void;
  } = $props();

  let plugins = $state<PluginSummary[]>([]);
  let loading = $state(true);
  let choosing = $state(false);
  let installing = $state(false);
  let uninstallingId = $state('');
  let sourceDir = $state('');
  let preview = $state<PluginPreview | null>(null);
  let allowSelectedFiles = $state(false);
  let acknowledgeEscalation = $state(false);
  let error = $state('');
  let message = $state('');

  const requestedRead = $derived(Boolean(preview?.requestedPermissions.includes('read-user-selected')));
  const hasEscalation = $derived(Boolean(preview?.additionalPermissions.length));
  const grantedPermissions = $derived<PluginPermissionGrant[]>(allowSelectedFiles && requestedRead ? ['read-user-selected'] : []);
  const networkRequest = $derived(preview?.manifest.toolManifest.permissions.network);
  const writeRequest = $derived(preview?.manifest.toolManifest.permissions.filesystem.write);
  const unsupportedPermissionRequest = $derived(Boolean(networkRequest && (networkRequest.mode !== 'none' || networkRequest.domains.length) || writeRequest && writeRequest !== 'none'));

  onMount(() => { void refresh(); });

  function messageOf(cause: unknown): string {
    if (typeof cause === 'string') return cause;
    if (cause instanceof Error) return cause.message;
    return 'The plugin service could not complete that request.';
  }

  async function refresh(): Promise<void> {
    loading = true;
    error = '';
    try {
      plugins = await listPlugins();
    } catch (cause) {
      error = messageOf(cause);
    } finally {
      loading = false;
    }
  }

  async function chooseAndPreview(): Promise<void> {
    choosing = true;
    error = '';
    message = '';
    try {
      const folder = await choosePluginPackage();
      if (!folder) return;
      sourceDir = folder;
      preview = await previewPlugin(folder);
      allowSelectedFiles = false;
      acknowledgeEscalation = false;
    } catch (cause) {
      preview = null;
      sourceDir = '';
      error = messageOf(cause);
    } finally {
      choosing = false;
    }
  }

  async function confirmInstall(): Promise<void> {
    if (!preview || !sourceDir || installing || unsupportedPermissionRequest || hasEscalation && !acknowledgeEscalation) return;
    installing = true;
    error = '';
    message = '';
    try {
      const installed = await installPlugin(sourceDir, grantedPermissions, acknowledgeEscalation);
      plugins = [...plugins.filter((plugin) => plugin.manifest.toolManifest.id !== installed.manifest.toolManifest.id), installed]
        .sort((a, b) => a.manifest.toolManifest.name.localeCompare(b.manifest.toolManifest.name));
      preview = null;
      sourceDir = '';
      allowSelectedFiles = false;
      acknowledgeEscalation = false;
      message = `${installed.manifest.toolManifest.name} installed. It is now available in search.`;
      onToolsChanged(await listTools());
    } catch (cause) {
      error = messageOf(cause);
    } finally {
      installing = false;
    }
  }

  async function removePlugin(plugin: PluginSummary): Promise<void> {
    const tool = plugin.manifest.toolManifest;
    uninstallingId = tool.id;
    error = '';
    message = '';
    try {
      await uninstallPlugin(tool.id);
      plugins = plugins.filter((item) => item.manifest.toolManifest.id !== tool.id);
      message = `${tool.name} removed.`;
      onToolsChanged(await listTools());
    } catch (cause) {
      error = messageOf(cause);
    } finally {
      uninstallingId = '';
    }
  }

  function declaredPermissions(plugin: PluginSummary | PluginPreview): string[] {
    const permissions = plugin.manifest.toolManifest.permissions;
    const result: string[] = [];
    if (permissions.filesystem.read === 'user-selected') result.push('Read user-selected files');
    if (permissions.filesystem.write !== 'none') result.push(`Write access: ${permissions.filesystem.write}`);
    if (permissions.network.mode !== 'none') {
      result.push(permissions.network.domains.length ? `Network: ${permissions.network.domains.join(', ')}` : 'Network access');
    }
    return result.length ? result : ['No filesystem or network access'];
  }

  function installedPermissionLabels(plugin: PluginSummary): string[] {
    const declaredRead = plugin.manifest.toolManifest.permissions.filesystem.read === 'user-selected';
    const grantedRead = plugin.grantedPermissions.includes('read-user-selected');
    const labels: string[] = [];
    if (declaredRead) labels.push(grantedRead ? 'Granted: reads files you select' : 'Selected-file reading is not granted');
    if (plugin.manifest.toolManifest.permissions.filesystem.write !== 'none') labels.push(`Declared write access: ${plugin.manifest.toolManifest.permissions.filesystem.write}`);
    if (plugin.manifest.toolManifest.permissions.network.mode !== 'none') labels.push(plugin.manifest.toolManifest.permissions.network.domains.length ? `Declared network access: ${plugin.manifest.toolManifest.permissions.network.domains.join(', ')}` : 'Declared network access');
    return labels.length ? labels : ['No host permissions granted'];
  }
</script>

<section class="plugin-manager" aria-label="Installed plugins and plugin installation">
  <div class="plugin-manager-toolbar"><p>Install sandboxed WASM tools from a local plugin package. Arcade Box shows the package identity and requested permissions before it installs.</p><button class="pipeline-primary-button" disabled={choosing} onclick={() => void chooseAndPreview()}><Icon name="plus" size={15} />{choosing ? 'Choosing…' : 'Choose plugin folder'}</button></div>
  {#if error}<div class="dashboard-alert" role="alert"><span class="alert-icon"><Icon name="shield" size={16} /></span><div><strong>Plugin operation failed</strong><span>{error}</span></div></div>{/if}
  {#if message}<div class="pipeline-message" role="status">{message}</div>{/if}

  {#if preview}
    <section class="plugin-review" aria-labelledby="plugin-review-heading">
      <div class="plugin-review-heading"><div><span class="eyebrow">{preview.installedVersion ? 'PLUGIN UPDATE REVIEW' : 'INSTALL REVIEW'}</span><h2 id="plugin-review-heading">{preview.manifest.toolManifest.name}</h2><p>{preview.manifest.toolManifest.description}{#if preview.installedVersion}<br />Installed version: {preview.installedVersion}{/if}</p></div><button class="quiet-button" onclick={() => { preview = null; sourceDir = ''; }}><Icon name="close" size={14} /><span>Cancel</span></button></div>
      <div class="plugin-identity-grid"><div><span>Author</span><strong>{preview.manifest.package.author}</strong></div><div><span>Version</span><strong>{preview.manifest.toolManifest.version}</strong></div><div><span>License</span><strong>{preview.manifest.package.license}</strong></div><div><span>Source</span><strong>{preview.manifest.package.source}</strong></div></div>
      <div class="plugin-permission-review"><div class="plugin-permission-heading"><Icon name="lock" size={16} /><div><strong>Requested access</strong><span>These permissions apply only to this installed plugin.</span></div></div>
        {#each declaredPermissions(preview) as permission}<div class="plugin-declared-permission"><span class="permission-status-dot"></span>{permission}</div>{/each}
        {#if requestedRead}<label class="plugin-grant-option"><input type="checkbox" bind:checked={allowSelectedFiles} /><span><strong>Allow reading files I select</strong><small>The plugin receives a scoped handle only for files chosen for its tool.</small></span></label>{/if}
        {#if !requestedRead}<p class="plugin-no-grant">This package requests no extra host grants.</p>{/if}
      </div>
      {#if hasEscalation}<label class="plugin-escalation-check"><input type="checkbox" bind:checked={acknowledgeEscalation} /><span>This update requests additional permissions. I have reviewed and acknowledge the change.</span></label>{/if}
      {#if unsupportedPermissionRequest}<div class="field-error" role="alert">This host currently rejects the requested network or filesystem write permissions. The package cannot be installed with those capabilities.</div>{/if}
      <div class="plugin-hash"><span>Component SHA-256</span><code>{preview.manifest.package.componentSha256}</code></div>
      <div class="plugin-review-actions"><button class="quiet-button" onclick={() => { preview = null; sourceDir = ''; }}>Cancel</button><button class="pipeline-primary-button" disabled={installing || unsupportedPermissionRequest || hasEscalation && !acknowledgeEscalation} onclick={() => void confirmInstall()}><Icon name="check" size={14} />{installing ? 'Installing…' : preview.installedVersion ? 'Update plugin' : 'Install plugin'}</button></div>
    </section>
  {/if}

  {#if loading}<div class="catalog-loading"><span class="spinner"></span><span>Loading installed plugins…</span></div>
  {:else if plugins.length === 0 && !preview}<div class="pipeline-empty"><span class="pipeline-empty-icon"><Icon name="shield" size={23} /></span><strong>No plugins installed</strong><span>Choose a local package folder to review and install an Arcade Box plugin.</span></div>{/if}

  {#if plugins.length > 0}
    <section class="catalog-section plugin-installed-section">
      <div class="catalog-section-heading"><div><h2>Installed plugins</h2><span>Each tool runs through the declared Arcade plugin interface</span></div><span class="section-count">{plugins.length}</span></div>
      <div class="plugin-list">
        {#each plugins as plugin (plugin.manifest.toolManifest.id)}
          {@const tool = plugin.manifest.toolManifest}
          <article class="plugin-card">
            <span class="plugin-card-icon"><Icon name="shield" size={18} /></span>
            <div class="plugin-card-copy"><strong>{tool.name}</strong><span>{tool.id} · v{tool.version} · {tool.category}</span><small>{tool.description}</small><div class="plugin-meta-tags"><span>{plugin.manifest.package.license}</span><span>{plugin.manifest.package.author}</span></div><div class="plugin-permission-tags">{#each installedPermissionLabels(plugin) as permission}<span><Icon name="lock" size={11} />{permission}</span>{/each}</div></div>
            <button class="quiet-button plugin-uninstall" disabled={uninstallingId === tool.id} onclick={() => void removePlugin(plugin)}><Icon name="close" size={14} /><span>{uninstallingId === tool.id ? 'Removing…' : 'Uninstall'}</span></button>
          </article>
        {/each}
      </div>
    </section>
  {/if}
</section>
