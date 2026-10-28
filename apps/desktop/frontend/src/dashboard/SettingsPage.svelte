<script lang="ts">
  // Settings in the standard sections and order (STANDARD §6): General,
  // Shortcuts, Clipboard history, Privacy, Connected apps, About.
  import { onMount } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import ConnectedApps from '../lib/ConnectedApps.svelte';
  import ShortcutRecorder from '../lib/ShortcutRecorder.svelte';
  import KeyChips from '../lib/KeyChips.svelte';
  import {
    aboutInfo, chooseSaveFolder, clearClipboardHistory, clipboardHistoryStatus, copyText, diagnostics, loginItem, openLogs,
    openReviewedUrl, openTools, saveFolder, setClipboardHistoryEnabled, setClipboardHistoryExclusions, setClipboardHistoryRetention,
    setLoginItem, setShortcut, shortcutOwner, type AboutInfo, type LoginItem,
  } from '../lib/arcade';
  import type { ClipboardHistoryStatus } from '../lib/contracts';
  import { privacyLabel } from '../lib/tool-utils';
  import { useBox } from '../box.svelte';

  const box = useBox();
  const DOCS = 'https://github.com/qa-p1/Arcade-box/blob/main/docs/user/index.md';
  const LICENCES = 'https://github.com/qa-p1/Arcade-box/blob/main/LICENSE';
  const TOOLS_RELEASES = 'https://github.com/qa-p1/Arcade-tools/releases/latest';
  const themes = [{ id: 'system', label: 'System' }, { id: 'light', label: 'Light' }, { id: 'dark', label: 'Dark' }] as const;
  const islandKeys = [
    { action: 'Run the tool', keys: ['Enter'] },
    { action: 'Next field', keys: ['Tab'] },
    { action: 'Back, or close the Island', keys: ['Esc'] },
    { action: 'Move through results', keys: ['↑', '↓'] },
    { action: 'Search tools in the dashboard', keys: [`${box.commandKey}+K`, '/'] },
  ];

  let login = $state<LoginItem | null>(null);
  let folder = $state<string | null>(null);
  let about = $state<AboutInfo | null>(null);
  let clipboard = $state<ClipboardHistoryStatus | null>(null);
  let exclusions = $state('');
  let confirmingClear = $state(false);
  let copied = $state(false);
  let busy = $state('');
  let error = $state('');
  const shortcut = $derived(box.shortcutInfo?.triggerDescription || box.shortcutInfo?.trigger_description || '');
  const shortcutState = $derived(box.shortcutInfo?.state.toLowerCase() ?? '');
  const onlineTools = $derived(box.runnableTools.filter((tool) => privacyLabel(tool.privacyClass) !== 'LOCAL'));

  function text(reason: unknown): string {
    return typeof reason === 'string' ? reason : reason instanceof Error ? reason.message : String(reason);
  }
  async function act(label: string, action: () => Promise<void>, failure: string): Promise<void> {
    busy = label;
    error = '';
    try { await action(); } catch (reason) { error = `${failure} ${text(reason)}`; } finally { busy = ''; }
  }

  onMount(() => {
    void loginItem().then((item) => { login = item; }).catch(() => {});
    void saveFolder().then((path) => { folder = path; }).catch(() => {});
    void aboutInfo().then((info) => { about = info; }).catch(() => {});
    void clipboardHistoryStatus().then((status) => { clipboard = status; exclusions = status.excludedApplications.join('\n'); }).catch(() => {});
  });

  async function applyShortcut(chord: string): Promise<void> {
    const status = await setShortcut(chord);
    box.acceptShortcutStatus(status);
    const state = status.state.toLowerCase();
    if (!['registered', 'starting', 'checking', 'updating'].includes(state)) throw new Error(status.message);
  }
  async function owners(chord: string): Promise<string[]> {
    const owner = await shortcutOwner(chord);
    return owner ? [owner] : [];
  }
  async function copyDiagnostics(): Promise<void> {
    await copyText(await diagnostics());
    copied = true;
    window.setTimeout(() => { copied = false; }, 1600);
  }
</script>

<header class="page-head">
  <div>
    <h1>Settings</h1>
    <p class="sub">Arcade Box on this device.</p>
  </div>
</header>

{#if error}<div class="notice danger" role="alert"><Icon name="alert" /><span class="row-main">{error}</span></div>{/if}

<section class="section" aria-labelledby="settings-general">
  <div class="section-head"><h2 id="settings-general">General</h2></div>
  <div class="list">
    <div class="row">
      <span class="row-main"><span class="row-title">Appearance</span></span>
      <div class="segmented" role="radiogroup" aria-label="Appearance">
        {#each themes as theme (theme.id)}
          <button type="button" role="radio" aria-checked={box.themeMode === theme.id} onclick={() => void act('theme', () => Promise.resolve(box.changeTheme(theme.id)), "Couldn't change the appearance.")}>{theme.label}</button>
        {/each}
      </div>
    </div>
    <label class="row switch-row" class:disabled={!login?.available}>
      <span class="row-main"><span class="row-title">Start at login</span>{#if login && !login.available}<span class="row-sub">{login.reason}</span>{:else}<span class="row-sub">Box waits in the background, ready for its shortcut.</span>{/if}</span>
      <input class="switch" type="checkbox" checked={login?.enabled ?? false} disabled={!login?.available || busy === 'login'} onchange={(event) => { const enabled = event.currentTarget.checked; void act('login', async () => { login = await setLoginItem(enabled); }, "Couldn't change start at login."); }} />
    </label>
    <div class="row">
      <span class="row-main"><span class="row-title">Save folder</span><span class="row-sub">Where Save… opens.</span>{#if folder}<span class="path save-folder" title={folder}>{folder}</span>{/if}</span>
      <div class="row-end">
        {#if folder}<button type="button" class="btn ghost sm" disabled={busy === 'folder'} onclick={() => void act('folder', async () => { folder = await chooseSaveFolder(true); }, "Couldn't reset the save folder.")}>Reset</button>{/if}
        <button type="button" class="btn sm" disabled={busy === 'folder'} onclick={() => void act('folder', async () => { folder = await chooseSaveFolder(); }, "Couldn't change the save folder.")}>{folder ? 'Change…' : 'Choose…'}</button>
      </div>
    </div>
    <div class="row">
      <span class="row-main"><span class="row-title">Welcome</span><span class="row-sub">Show the first-run steps in the Island again.</span></span>
      <button type="button" class="btn sm" onclick={() => void act('welcome', box.showWelcomeAgain, "Couldn't show the welcome.")}>Show again</button>
    </div>
  </div>
</section>

<section class="section" aria-labelledby="settings-shortcuts">
  <div class="section-head"><h2 id="settings-shortcuts">Shortcuts</h2></div>
  <div class="list">
    <div class="row stacked">
      <div class="shortcut-head">
        <span class="row-main"><span class="row-title">Show or hide the Island</span><span class="row-sub">Works from any app.</span></span>
        <ShortcutRecorder value={shortcut} label="Show or hide the Island" defaultValue={box.defaultShortcutForPlatform()} conflictsFor={owners} onApply={applyShortcut} />
      </div>
      {#if box.shortcutInfo && shortcutState !== 'registered'}<p class="status-warning">{box.shortcutInfo.message}</p>{/if}
      {#if box.shortcutInfo?.backend.toLowerCase().includes('portal') || box.shortcutInfo?.backend.toLowerCase().includes('wayland')}<p class="row-sub">Your desktop may ask to allow the shortcut the first time.</p>{/if}
    </div>
  </div>
  <div class="section-head island-keys-head"><h3>Island keys</h3></div>
  <div class="list">
    {#each islandKeys as entry (entry.action)}
      <div class="row"><span class="row-main"><span class="row-title">{entry.action}</span></span><KeyChips keys={entry.keys} /></div>
    {/each}
  </div>
</section>

<section class="section" aria-labelledby="settings-clipboard">
  <div class="section-head"><h2 id="settings-clipboard">Clipboard history</h2></div>
  <div class="list">
    <label class="row switch-row">
      <span class="row-main"><span class="row-title">Keep clipboard history</span><span class="row-sub">{clipboard?.enabled ? `${clipboard.itemCount} ${clipboard.itemCount === 1 ? 'item' : 'items'} saved on this device. Password-like text is skipped.` : 'Off: Box reads the clipboard only when you open it.'}</span></span>
      <input class="switch" type="checkbox" checked={clipboard?.enabled ?? false} disabled={!clipboard || busy === 'clipboard'} onchange={(event) => { const enabled = event.currentTarget.checked; void act('clipboard', async () => { clipboard = await setClipboardHistoryEnabled(enabled); }, "Couldn't change clipboard history."); }} />
    </label>
    <div class="row">
      <span class="row-main"><span class="row-title">Keep items for</span></span>
      <select class="input select sm" aria-label="Keep items for" value={String(clipboard?.retentionDays ?? 7)} disabled={!clipboard} onchange={(event) => { const days = Number(event.currentTarget.value); void act('clipboard', async () => { clipboard = await setClipboardHistoryRetention(days); }, "Couldn't change how long items are kept."); }}>
        <option value="1">1 day</option><option value="7">7 days</option><option value="30">30 days</option><option value="90">90 days</option>
      </select>
    </div>
    <div class="row stacked">
      <span class="row-main"><span class="row-title">Skip these apps</span><span class="row-sub">{clipboard?.sourceApplicationAvailable ? 'One app name per line. Nothing copied in them is saved.' : "This desktop doesn't say which app copied something, so this list can't apply here."}</span></span>
      <textarea class="input" rows="3" bind:value={exclusions} spellcheck="false" aria-label="Apps to skip" disabled={!clipboard}></textarea>
      <div class="btn-group"><button type="button" class="btn sm" disabled={!clipboard || busy === 'exclusions'} onclick={() => void act('exclusions', async () => { clipboard = await setClipboardHistoryExclusions(exclusions.split(/[\n,]/).map((name) => name.trim()).filter(Boolean)); }, "Couldn't save the list.")}>Save list</button></div>
    </div>
    <div class="row">
      <span class="row-main"><span class="row-title">Clear history</span><span class="row-sub">Pinned items go too.</span></span>
      <button type="button" class="btn sm danger" disabled={!clipboard?.itemCount} onclick={() => { confirmingClear = true; }}>Clear…</button>
    </div>
  </div>
</section>

<section class="section" aria-labelledby="settings-privacy">
  <div class="section-head"><h2 id="settings-privacy">Privacy</h2></div>
  <div class="list">
    <div class="row">
      <span class="row-main"><span class="row-title">Tools run on this device</span><span class="row-sub">Files, text and results stay here unless a tool says it goes online.</span></span>
    </div>
    <div class="row stacked">
      <span class="row-main"><span class="row-title">Tools that go online</span><span class="row-sub">These reach a website or an online service to do their job.</span></span>
      {#if onlineTools.length}
        <ul class="online-tools">{#each onlineTools as tool (tool.id)}<li><span class="ellipsis" title={tool.name}>{tool.name}</span><span class="badge warn">online</span></li>{/each}</ul>
      {/if}
    </div>
    <div class="row stacked">
      <span class="row-main"><span class="row-title">API keys</span><span class="row-sub">Cloud transcription reads GROQ_API_KEY from the environment or from the .env file in Box's data folder. Keys are never shown, logged or sent anywhere else.</span></span>
      {#if about?.dataFolder}<span class="path">{about.dataFolder}</span>{/if}
    </div>
  </div>
</section>

<section class="section" aria-labelledby="settings-connected">
  <div class="section-head"><h2 id="settings-connected">Connected apps</h2></div>
  <ConnectedApps />
</section>

<section class="section" aria-labelledby="settings-about">
  <div class="section-head"><h2 id="settings-about">About</h2></div>
  <div class="list">
    <div class="row"><span class="row-main"><span class="row-title">Arcade Box {about?.version ?? box.version}</span><span class="row-sub">{about ? `${about.channel[0].toUpperCase()}${about.channel.slice(1)} channel` : ''}</span></span></div>
    <div class="row">
      {#if about?.updatesByTools}
        <span class="row-main"><span class="row-title">Updates are managed by Arcade Tools</span></span>
        <button type="button" class="btn sm" onclick={() => void act('tools', openTools, "Couldn't open Arcade Tools.")}>Open Tools</button>
      {:else}
        <span class="row-main"><span class="row-title">Updates</span><span class="row-sub">Arcade Tools installs and updates every Arcade app.</span></span>
        <button type="button" class="btn sm" onclick={() => void act('updates', () => openReviewedUrl(TOOLS_RELEASES), "Couldn't open the link.")}>Check for updates</button>
      {/if}
    </div>
    <div class="row"><span class="row-main"><span class="row-title">User guide</span></span><button type="button" class="btn sm" onclick={() => void act('guide', () => openReviewedUrl(DOCS), "Couldn't open the guide.")}>Open user guide</button></div>
    <div class="row"><span class="row-main"><span class="row-title">Logs</span></span><button type="button" class="btn sm" onclick={() => void act('logs', openLogs, "Couldn't open the logs.")}>Open logs folder</button></div>
    <div class="row"><span class="row-main"><span class="row-title">Diagnostics</span><span class="row-sub">Versions, desktop and connected apps, for a bug report. No files or keys.</span></span><button type="button" class="btn sm" disabled={busy === 'diagnostics'} onclick={() => void act('diagnostics', copyDiagnostics, "Couldn't copy the diagnostics.")}><Icon name={copied ? 'check' : 'copy'} size={14} />{copied ? 'Copied' : 'Copy diagnostics'}</button></div>
    <div class="row"><span class="row-main"><span class="row-title">Licences</span><span class="row-sub">GPL-3.0-or-later, with the licences of the libraries Box uses.</span></span><button type="button" class="btn sm" onclick={() => void act('licences', () => openReviewedUrl(LICENCES), "Couldn't open the licences.")}>Licences</button></div>
  </div>
</section>

{#if confirmingClear}
  <div class="dialog-backdrop" role="presentation" onclick={() => { confirmingClear = false; }}>
    <div class="card dialog" role="alertdialog" aria-modal="true" aria-labelledby="clear-title" aria-describedby="clear-text" tabindex="-1" onclick={(event) => event.stopPropagation()} onkeydown={(event) => { if (event.key === 'Escape') confirmingClear = false; }}>
      <h2 id="clear-title" class="t-heading">Clear clipboard history?</h2>
      <p id="clear-text" class="muted">All {clipboard?.itemCount ?? 0} saved items are deleted, pinned ones too. What's on the clipboard now stays.</p>
      <div class="dialog-actions">
        <button type="button" class="btn" onclick={() => { confirmingClear = false; }}>Keep</button>
        <!-- svelte-ignore a11y_autofocus -->
        <button type="button" class="btn destructive" autofocus onclick={() => { confirmingClear = false; void act('clear', async () => { await clearClipboardHistory(); clipboard = await clipboardHistoryStatus(); }, "Couldn't clear the history."); }}>Delete history</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .notice { margin-bottom: var(--sp-16); }
  .switch-row.disabled { cursor: default; }
  .save-folder { display: block; margin-top: var(--sp-4); color: var(--faint); }
  .shortcut-head { display: flex; flex-wrap: wrap; align-items: center; gap: var(--sp-12); }
  .shortcut-head .row-main { flex: 1 1 200px; }
  .island-keys-head { margin-top: var(--sp-20); }
  .island-keys-head h3 { font-size: var(--fs-body); font-weight: var(--fw-semibold); }
  .row.stacked { flex-direction: column; align-items: stretch; gap: var(--sp-8); }
  .row textarea { width: 100%; min-height: 72px; resize: vertical; font-family: var(--font-mono); font-size: var(--fs-mono); }
  .online-tools { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: var(--sp-6) var(--sp-16); margin: 0; padding: 0; list-style: none; }
  .online-tools li { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-8); min-width: 0; }
  .dialog-backdrop { position: fixed; inset: 0; z-index: 40; display: grid; place-items: center; padding: var(--sp-16); background: var(--scrim); }
  .dialog { width: min(420px, 100%); box-shadow: var(--shadow-pop); }
  .dialog p { margin-top: var(--sp-8); }
</style>
