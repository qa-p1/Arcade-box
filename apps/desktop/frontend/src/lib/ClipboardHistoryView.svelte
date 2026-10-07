<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icon.svelte';
  import ClipboardImagePreview from './ClipboardImagePreview.svelte';
  import { clearClipboardHistory, clipboardHistoryStatus, copyClipboardHistoryItem, deleteClipboardHistoryItem, listClipboardHistory, pinClipboardHistoryItem, setClipboardHistoryEnabled, setClipboardHistoryExclusions, setClipboardHistoryRetention } from './arcade';
  import type { ClipboardHistoryItem, ClipboardHistoryStatus } from './contracts';

  let status = $state<ClipboardHistoryStatus | null>(null);
  let items = $state<ClipboardHistoryItem[]>([]);
  let query = $state('');
  let exclusionText = $state('');
  let busy = $state(false);
  let message = $state('');
  let error = $state('');
  let searchTimer: number | undefined;

  onMount(() => {
    void refresh();
    return () => {
      if (searchTimer !== undefined) window.clearTimeout(searchTimer);
    };
  });

  $effect(() => {
    query;
    if (searchTimer !== undefined) window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => void refreshItems(), 180);
  });

  async function refresh(): Promise<void> {
    error = '';
    try {
      status = await clipboardHistoryStatus();
      exclusionText = status.excludedApplications.join('\n');
      items = await listClipboardHistory(query);
    } catch (cause) {
      error = messageOf(cause);
    }
  }

  async function refreshItems(): Promise<void> {
    try {
      items = await listClipboardHistory(query);
    } catch (cause) {
      error = messageOf(cause);
    }
  }

  async function toggleHistory(): Promise<void> {
    if (!status || busy) return;
    busy = true;
    error = '';
    message = '';
    try {
      status = await setClipboardHistoryEnabled(!status.enabled);
      message = status.enabled ? 'Clipboard history is on.' : 'Clipboard history is off.';
      await refreshItems();
    } catch (cause) {
      error = messageOf(cause);
    } finally {
      busy = false;
    }
  }

  async function changeRetention(value: string): Promise<void> {
    const days = Number(value);
    if (!Number.isInteger(days)) return;
    try {
      status = await setClipboardHistoryRetention(days);
      await refreshItems();
    } catch (cause) {
      error = messageOf(cause);
    }
  }

  async function saveExclusions(): Promise<void> {
    try {
      const applications = exclusionText.split(/[\n,]/).map((name) => name.trim()).filter(Boolean);
      status = await setClipboardHistoryExclusions(applications);
      message = 'Application exclusions saved.';
    } catch (cause) {
      error = messageOf(cause);
    }
  }

  async function togglePin(item: ClipboardHistoryItem): Promise<void> {
    try {
      await pinClipboardHistoryItem(item.id, !item.pinned);
      await refresh();
    } catch (cause) {
      error = messageOf(cause);
    }
  }

  async function copyItem(item: ClipboardHistoryItem): Promise<void> {
    try {
      await copyClipboardHistoryItem(item.id);
      message = item.kind === 'image' ? 'Image copied to the clipboard.' : 'Item copied to the clipboard.';
    } catch (cause) {
      error = messageOf(cause);
    }
  }

  async function deleteItem(item: ClipboardHistoryItem): Promise<void> {
    try {
      await deleteClipboardHistoryItem(item.id);
      await refresh();
    } catch (cause) {
      error = messageOf(cause);
    }
  }

  async function clearAll(): Promise<void> {
    try {
      await clearClipboardHistory();
      items = [];
      message = 'Clipboard history cleared.';
      await refresh();
    } catch (cause) {
      error = messageOf(cause);
    }
  }

  function messageOf(cause: unknown): string {
    return typeof cause === 'string' ? cause : cause instanceof Error ? cause.message : 'Clipboard history could not be updated.';
  }

  function capturedAt(seconds: number): string {
    return new Date(seconds * 1000).toLocaleString();
  }
</script>

<section class="clipboard-history-view" aria-label="Clipboard history">
  <header class="clipboard-history-head">
    <div><strong>{status?.enabled ? 'History is on' : 'History is off'}</strong><span>{status?.message || 'Checking clipboard history settings…'}</span></div>
    <button class="clipboard-toggle" class:clipboard-toggle-on={status?.enabled} type="button" disabled={busy || !status} aria-pressed={status?.enabled || false} onclick={() => void toggleHistory()}>
      <Icon name={status?.enabled ? 'check' : 'close'} size={14} />{busy ? 'Saving…' : status?.enabled ? 'Turn off' : 'Turn on'}
    </button>
  </header>

  <div class="clipboard-history-settings">
    <label for="clipboard-history-search">Search history</label>
    <input id="clipboard-history-search" type="search" bind:value={query} placeholder="Filter saved clipboard items…" autocomplete="off" />
    <label for="clipboard-history-retention">Keep items for</label>
    <select id="clipboard-history-retention" value={status?.retentionDays || 7} onchange={(event) => void changeRetention((event.currentTarget as HTMLSelectElement).value)}>
      <option value="1">1 day</option><option value="7">7 days</option><option value="30">30 days</option><option value="90">90 days</option>
    </select>
    <button class="quiet-button clipboard-clear" type="button" disabled={!items.length} onclick={() => void clearAll()}><Icon name="trash" size={14} /><span>Clear history</span></button>
  </div>

  <details class="clipboard-exclusion-settings">
    <summary>Privacy exclusions</summary>
    <label for="clipboard-exclusions">Application names, one per line</label>
    <textarea id="clipboard-exclusions" bind:value={exclusionText} placeholder="Password Manager&#10;Secure Wallet" spellcheck="false"></textarea>
    <small>{status?.sourceApplicationAvailable ? 'Arcade Box checks the active application when this desktop exposes it. Wayland does not provide this information.' : 'Source app detection is unavailable in this session, so application exclusions cannot be applied. X11 needs xdotool; Wayland does not expose the active app.'}</small>
    <button type="button" class="quiet-button" onclick={() => void saveExclusions()}><Icon name="check" size={13} /><span>Save exclusions</span></button>
  </details>

  {#if error}<div class="field-error" role="alert">{error}</div>{/if}
  {#if message}<p class="clipboard-feedback" role="status">{message}</p>{/if}

  {#if !status?.enabled}
    <div class="clipboard-history-empty"><Icon name="lock" size={20} /><strong>Turn history on when you want it</strong><span>Clipboard content is read only while this opt-in is enabled. Password-like content is skipped automatically.</span></div>
  {:else if items.length === 0}
    <div class="clipboard-history-empty"><Icon name="clipboard" size={20} /><strong>No matching clipboard items</strong><span>Copy text or an image in another app. New items appear here while Arcade Box is running.</span></div>
  {:else}
    <div class="clipboard-history-items" aria-label="Saved clipboard items">
      {#each items as item (item.id)}
        <article class="clipboard-history-item">
          <header><span class="clipboard-kind"><Icon name={item.kind === 'image' ? 'image' : item.kind === 'file' ? 'file' : 'clipboard'} size={13} />{item.kind === 'image' ? 'Image' : item.kind === 'file' ? 'File reference' : 'Text'}</span><time>{capturedAt(item.capturedAt)}</time></header>
          {#if item.kind === 'image' && item.imageRgbaBase64 && item.imageWidth && item.imageHeight}
            <ClipboardImagePreview rgbaBase64={item.imageRgbaBase64} width={item.imageWidth} height={item.imageHeight} />
          {:else if item.text}
            <details class="clipboard-text-content"><summary>{item.preview || '(empty)'}</summary><pre>{item.text}</pre></details>
          {:else}
            <span class="clipboard-text-preview">{item.preview}</span>
          {/if}
          {#if item.sourceApplication}<small class="clipboard-source">Copied from {item.sourceApplication}</small>{/if}
          <div class="clipboard-item-actions">
            <button type="button" class="quiet-button" onclick={() => void copyItem(item)}><Icon name="copy" size={13} /><span>Copy</span></button>
            <button type="button" class="quiet-button" aria-pressed={item.pinned} onclick={() => void togglePin(item)}><Icon name="pin" size={13} /><span>{item.pinned ? 'Unpin' : 'Pin'}</span></button>
            <button type="button" class="quiet-button" onclick={() => void deleteItem(item)}><Icon name="trash" size={13} /><span>Delete</span></button>
          </div>
        </article>
      {/each}
    </div>
  {/if}
</section>

<style>
  .clipboard-history-view { display:grid; gap:12px; }
  .clipboard-history-head { display:flex; align-items:center; justify-content:space-between; gap:10px; padding:10px 11px; border:1px solid var(--border-subtle); border-radius:10px; background:var(--panel-subtle); }
  .clipboard-history-head > div { display:grid; gap:3px; }
  .clipboard-history-head strong { font-size:12px; }
  .clipboard-history-head span { color:var(--text-muted); font-size:10px; }
  .clipboard-toggle { display:inline-flex; align-items:center; gap:6px; min-height:31px; padding:0 10px; border:1px solid var(--border-subtle); border-radius:8px; background:transparent; color:var(--text-secondary); font:inherit; font-size:10px; cursor:pointer; }
  .clipboard-toggle-on { border-color:rgba(141,119,209,.45); background:rgba(141,119,209,.12); color:var(--text-primary); }
  .clipboard-history-settings { display:grid; grid-template-columns:1fr auto; align-items:center; gap:7px 9px; }
  .clipboard-history-settings label { color:var(--text-muted); font-size:10px; }
  .clipboard-history-settings input { grid-column:1 / -1; width:100%; min-height:34px; padding:7px 9px; border:1px solid var(--border-subtle); border-radius:8px; background:var(--panel-subtle); color:var(--text-primary); font:inherit; font-size:11px; }
  .clipboard-history-settings select { min-width:90px; min-height:30px; border:1px solid var(--border-subtle); border-radius:7px; background:var(--panel-subtle); color:var(--text-primary); font:inherit; font-size:10px; }
  .clipboard-clear { grid-column:1 / -1; justify-self:start; }
  .clipboard-exclusion-settings { display:grid; gap:7px; padding:8px 9px; border:1px solid var(--border-subtle); border-radius:9px; }
  .clipboard-exclusion-settings summary { color:var(--text-secondary); font-size:10px; cursor:pointer; }
  .clipboard-exclusion-settings label, .clipboard-exclusion-settings small { color:var(--text-muted); font-size:9px; }
  .clipboard-exclusion-settings textarea { min-height:48px; padding:7px; border:1px solid var(--border-subtle); border-radius:7px; background:var(--panel-subtle); color:var(--text-primary); font:inherit; font-size:10px; resize:vertical; }
  .clipboard-feedback { margin:0; color:var(--text-muted); font-size:10px; }
  .clipboard-history-empty { display:grid; justify-items:center; gap:5px; padding:22px 14px; border:1px dashed var(--border-subtle); border-radius:10px; color:var(--text-muted); text-align:center; }
  .clipboard-history-empty strong { color:var(--text-primary); font-size:11px; }
  .clipboard-history-empty span { max-width:380px; font-size:10px; line-height:1.5; }
  .clipboard-history-items { display:grid; gap:8px; max-height:330px; overflow:auto; }
  .clipboard-history-item { display:grid; gap:7px; padding:9px; border:1px solid var(--border-subtle); border-radius:9px; background:var(--panel-subtle); }
  .clipboard-history-item > header { display:flex; justify-content:space-between; align-items:center; color:var(--text-muted); font-size:9px; }
  .clipboard-kind { display:inline-flex; align-items:center; gap:5px; text-transform:uppercase; letter-spacing:.05em; }
  .clipboard-text-content summary { overflow:hidden; color:var(--text-primary); font-size:10px; text-overflow:ellipsis; white-space:nowrap; cursor:pointer; }
  .clipboard-text-content pre { max-height:160px; margin:7px 0 0; overflow:auto; white-space:pre-wrap; overflow-wrap:anywhere; color:var(--text-secondary); font:10px/1.5 ui-monospace,monospace; }
  .clipboard-text-preview { overflow:hidden; color:var(--text-primary); font-size:10px; text-overflow:ellipsis; }
  .clipboard-source { color:var(--text-muted); font-size:9px; }
  .clipboard-item-actions { display:flex; flex-wrap:wrap; gap:3px; }
  .clipboard-item-actions button { min-height:27px; padding:0 7px; font-size:9px; }
  @media (max-width:560px) { .clipboard-history-head { align-items:flex-start; flex-direction:column; } }
</style>
