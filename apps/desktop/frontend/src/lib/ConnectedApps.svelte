<script lang="ts">
  import { onMount } from 'svelte';
  import ArcadeBadge from './ArcadeBadge.svelte';
  import { connectedApps, getConnectedApp, setLinkSettings, watchLinkChanged, type ConnectedAppsState, type LinkSettings } from './arcade';
  let connections = $state<ConnectedAppsState | null>(null);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state('');
  let disposed = false;
  let generation = 0;
  function errorText(reason: unknown): string { return typeof reason === 'string' ? reason : reason instanceof Error ? reason.message : 'Could not update connected apps.'; }
  async function load(): Promise<void> {
    const current = ++generation;
    try { const value = await connectedApps(); if (!disposed && current === generation) connections = value; }
    catch (reason) { if (!disposed) error = errorText(reason); }
    finally { if (!disposed) loading = false; }
  }
  async function save(settings: LinkSettings): Promise<void> {
    busy = true; error = '';
    try { await setLinkSettings(settings); await load(); }
    catch (reason) { error = errorText(reason); }
    finally { busy = false; }
  }
  async function get(id: string): Promise<void> {
    busy = true; error = '';
    try { await getConnectedApp(id); } catch (reason) { error = errorText(reason); } finally { busy = false; }
  }
  onMount(() => {
    let stop = () => {};
    void watchLinkChanged(() => void load()).then((unlisten) => { if (disposed) unlisten(); else stop = unlisten; });
    void load();
    return () => { disposed = true; generation++; stop(); };
  });
</script>

<section class="settings-card connected-apps" aria-labelledby="connected-heading">
  <div class="settings-card-heading"><ArcadeBadge app="arcade.box" /><div><h2 id="connected-heading">Connected apps</h2><p>Choose which Arcade apps work with Box.</p></div></div>
  {#if loading}<p role="status">Loading connected apps…</p>
  {:else if connections}
    <label class="connection-master"><input type="checkbox" checked={connections.settings.enabled} disabled={busy} onchange={(event) => void save({ ...connections!.settings, enabled: event.currentTarget.checked })} /><strong>Connect with other Arcade apps</strong></label>
    {#each connections.apps as peer (peer.id)}
      <div class="peer-row"><ArcadeBadge app={peer.id} />
        <div class="peer-copy"><strong>{peer.name}</strong><span>{peer.state}{peer.version ? ` · v${peer.version}` : ''}</span>{#if peer.state === 'Not installed'}<p>{peer.pitch}</p>{/if}</div>
        {#if peer.state === 'Not installed'}<button class="quiet-button" disabled={busy} onclick={() => void get(peer.id)}>Get</button>
        {:else}<label class="peer-toggle"><input type="checkbox" checked={peer.enabled} disabled={busy || !connections.settings.enabled} onchange={(event) => void save({ ...connections!.settings, disabledPeers: event.currentTarget.checked ? connections!.settings.disabledPeers.filter((id) => id !== peer.id) : [...connections!.settings.disabledPeers.filter((id) => id !== peer.id), peer.id] })} />Use with Box</label>{/if}
      </div>
    {/each}
    <details class="connection-diagnostics"><summary>Diagnostics</summary><dl><dt>Registry</dt><dd>{connections.registryPath}</dd><dt>Box endpoint</dt><dd>{connections.endpointState}</dd><dt>Last error</dt><dd>{connections.lastError || 'None'}</dd>{#each connections.apps as peer}<dt>{peer.name} endpoint</dt><dd>{peer.state} · {peer.endpoint}</dd>{/each}</dl></details>
  {:else}<p>Open the desktop app to manage connections.</p>{/if}
  {#if error}<p class="field-error" role="alert">{error}</p>{/if}
</section>

<style>
  .connected-apps { border-radius: 12px; }
  .connection-master { display: flex; gap: 10px; align-items: center; padding: 12px 0; }
  .peer-row { display: flex; gap: 12px; align-items: center; padding: 14px 0; border-top: 1px solid var(--line); }
  .peer-copy { flex: 1; display: grid; gap: 4px; }
  .peer-copy span, .peer-copy p { font-size: 12px; color: var(--muted); margin: 0; }
  .peer-toggle { display: flex; align-items: center; gap: 8px; font-size: 12px; }
  .connection-diagnostics { margin-top: 12px; color: var(--muted); font-size: 12px; }
  summary { cursor: pointer; }
  dl { display: grid; gap: 6px; }
  dt { font-weight: 600; }
  dd { margin: 0 0 6px; overflow-wrap: anywhere; }
</style>
