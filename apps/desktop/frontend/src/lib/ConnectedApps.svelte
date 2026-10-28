<script lang="ts">
  // Settings › Connected apps, the shared Link page (STANDARD §6): the
  // master switch, then one row per other Arcade app. Box keeps its per-app
  // switches so a connection can be turned off for one app only.
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import { connectedApps, getConnectedApp, openReviewedUrl, setLinkSettings, watchLinkChanged, type ConnectedApp, type ConnectedAppsState, type LinkSettings } from './arcade';

  const glyphs = import.meta.glob<string>('../design/glyphs/*.svg', { eager: true, query: '?url', import: 'default' });
  const glyph = (id: string) => glyphs[`../design/glyphs/${id}.svg`] ?? glyphs['../design/glyphs/arcade.box.svg'];
  const GUIDE = 'https://github.com/qa-p1/Arcade-box/blob/main/docs/user/with-other-apps.md';

  let connections = $state<ConnectedAppsState | null>(null);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let disposed = false;
  let generation = 0;
  function errorText(reason: unknown): string { return typeof reason === 'string' ? reason : reason instanceof Error ? reason.message : String(reason); }
  async function load(): Promise<void> {
    const current = ++generation;
    try { const value = await connectedApps(); if (!disposed && current === generation) { connections = value; error = ''; } }
    catch (reason) { if (!disposed) error = `Couldn't load the connected apps. ${errorText(reason)}`; }
    finally { if (!disposed) loading = false; }
  }
  async function save(settings: LinkSettings): Promise<void> {
    busy = true; error = '';
    try { await setLinkSettings(settings); await load(); }
    catch (reason) { error = `Couldn't change the connection. ${errorText(reason)}`; }
    finally { busy = false; }
  }
  async function install(id: string): Promise<void> {
    busy = true; error = '';
    try { await getConnectedApp(id); } catch (reason) { error = `Couldn't ask Arcade Tools to install it. ${errorText(reason)}`; } finally { busy = false; }
  }
  function setPeer(settings: LinkSettings, peer: ConnectedApp, enabled: boolean): LinkSettings {
    const others = settings.disabledPeers.filter((id) => id !== peer.id);
    return { ...settings, disabledPeers: enabled ? others : [...others, peer.id] };
  }
  const status = (peer: ConnectedApp) => peer.state === 'Running' ? { tone: 'ok', text: 'Connected' } : peer.state === 'Installed' ? { tone: '', text: 'Not running' } : { tone: '', text: 'Not installed' };
  onMount(() => {
    let stop = () => {};
    void watchLinkChanged(() => void load()).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    void load();
    return () => { disposed = true; generation++; stop(); };
  });
</script>

{#if loading}
  <div class="list" aria-busy="true" aria-label="Loading connected apps">{#each [0, 1, 2] as item (item)}<div class="skeleton-row"><span class="glyph-tile skeleton"></span><div class="row-main"><div class="skeleton line"></div></div></div>{/each}</div>
{:else if connections}
  {@const settings = connections.settings}
  {#if !settings.enabled}
    <div class="notice warn"><Icon name="alert" /><span class="row-main">Connections are off in this app.</span><button type="button" class="btn sm" disabled={busy} onclick={() => void save({ ...settings, enabled: true })}>Turn on</button></div>
  {/if}
  <div class="list">
    <label class="row switch-row">
      <span class="row-main"><span class="row-title">Connect with other Arcade apps</span><span class="row-sub">Send results to other apps and take files and text from them.</span></span>
      <input class="switch" type="checkbox" role="switch" checked={settings.enabled} disabled={busy} onchange={(event) => void save({ ...settings, enabled: event.currentTarget.checked })} />
    </label>
    {#each connections.apps as peer (peer.id)}
      {@const state = status(peer)}
      <div class="row">
        <span class="glyph-tile"><img src={glyph(peer.id)} alt="" width="18" height="18" /></span>
        <div class="row-main">
          <div class="row-title">{peer.name}</div>
          <div class="row-sub">{peer.pitch}</div>
        </div>
        <span class="badge {state.tone}">{#if state.tone === 'ok'}<span class="dot ok"></span>{/if}{state.text}</span>
        {#if peer.state === 'Not installed'}
          <button type="button" class="btn sm" disabled={busy} onclick={() => void install(peer.id)}>Install</button>
        {:else}
          <input class="switch" type="checkbox" role="switch" aria-label="Use {peer.name} with Arcade Box" checked={peer.enabled} disabled={busy || !settings.enabled} onchange={(event) => void save(setPeer(settings, peer, event.currentTarget.checked))} />
        {/if}
      </div>
    {/each}
  </div>
  <details class="connection-details">
    <summary>Connection details</summary>
    <dl>
      <dt>Box</dt><dd>{connections.endpointState}</dd>
      {#each connections.apps as peer (peer.id)}<dt>{peer.name}</dt><dd>{peer.state}{peer.version ? ` ${peer.version}` : ''}</dd>{/each}
      {#if connections.lastError}<dt>Last error</dt><dd>{connections.lastError}</dd>{/if}
    </dl>
  </details>
  <button type="button" class="link-button how-it-works" onclick={() => void openReviewedUrl(GUIDE).catch(() => {})}>How it works</button>
{:else}
  <p class="section-note">Open the desktop app to manage connections.</p>
{/if}
{#if error}<div class="notice danger" role="alert"><Icon name="alert" /><span class="row-main">{error}</span><button type="button" class="btn sm" onclick={() => void load()}>Retry</button></div>{/if}

<style>
  .notice { margin-bottom: var(--sp-12); }
  .notice.danger { margin: var(--sp-12) 0 0; }
  .connection-details { margin-top: var(--sp-12); color: var(--muted); font-size: var(--fs-small); }
  .connection-details summary { cursor: pointer; }
  dl { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: var(--sp-4) var(--sp-12); margin: var(--sp-8) 0 0; }
  dt { font-weight: var(--fw-semibold); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .how-it-works { margin-top: var(--sp-8); }
</style>
