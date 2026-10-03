<script lang="ts">
  import { onDestroy } from 'svelte';
  import { copyImageResult, imageResultPreview, openArtifact, revealArtifact, saveArtifactAs } from './arcade';
  import type { ImageResultPreview, ToolOutput } from './contracts';
  import Icon from './Icon.svelte';

  let {
    output,
    name,
    size = '',
    busy = false,
    onSave,
    onOpen,
    onReveal,
  }: {
    output: ToolOutput;
    name: string;
    size?: string;
    busy?: boolean;
    onSave?: () => void | Promise<unknown>;
    onOpen?: () => void | Promise<unknown>;
    onReveal?: () => void | Promise<unknown>;
  } = $props();

  let preview = $state<ImageResultPreview | null>(null);
  let previewError = $state('');
  let copying = $state(false);
  let copyStatus = $state('');
  let actionError = $state('');
  let actionBusy = $state(false);
  let copyTimer: number | undefined;

  onDestroy(() => {
    if (copyTimer !== undefined) window.clearTimeout(copyTimer);
  });

  $effect(() => {
    const token = output.value;
    let current = true;
    preview = null;
    previewError = '';
    if (!token) return;
    void imageResultPreview(token)
      .then((result) => {
        if (current) preview = result;
      })
      .catch((cause: unknown) => {
        if (current) previewError = cause instanceof Error ? cause.message : String(cause);
      });
    return () => {
      current = false;
    };
  });

  function message(cause: unknown): string {
    return typeof cause === 'string' ? cause : cause instanceof Error ? cause.message : 'The image action failed.';
  }

  async function copyImage(): Promise<void> {
    if (copying || busy) return;
    copying = true;
    copyStatus = '';
    actionError = '';
    try {
      await copyImageResult(output.value);
      copyStatus = 'Image copied';
      if (copyTimer !== undefined) window.clearTimeout(copyTimer);
      copyTimer = window.setTimeout(() => {
        copyStatus = '';
        copyTimer = undefined;
      }, 1600);
    } catch (cause) {
      actionError = message(cause);
    } finally {
      copying = false;
    }
  }

  async function perform(action: 'save' | 'open' | 'reveal'): Promise<void> {
    if (actionBusy || busy) return;
    actionBusy = true;
    actionError = '';
    try {
      if (action === 'save') {
        if (onSave) await onSave();
        else await saveArtifactAs(output.value);
      } else if (action === 'open') {
        if (onOpen) await onOpen();
        else await openArtifact(output.value);
      } else if (onReveal) {
        await onReveal();
      } else {
        await revealArtifact(output.value);
      }
    } catch (cause) {
      actionError = message(cause);
    } finally {
      actionBusy = false;
    }
  }
</script>

<article class="image-result" aria-label="Image result">
  <div class="image-result-preview">
    {#if preview}
      <img src={preview.dataUrl} alt={`${name} preview`} draggable="false" />
    {:else if previewError}
      <span role="status">Preview unavailable</span>
    {:else}
      <span class="image-result-spinner" aria-label="Loading image preview"></span>
    {/if}
  </div>
  <div class="image-result-main">
    <div class="image-result-copy">
      <strong title={name}>{name}</strong>
      <span>{#if size}{size} · {/if}{#if preview}{preview.width} × {preview.height} px · {/if}{output.mime}</span>
    </div>
    <div class="image-result-actions">
      <button type="button" class="quiet-button" data-copy-image disabled={busy || copying} onclick={() => void copyImage()}>
        <Icon name={copyStatus ? 'check' : 'copy'} size={13} /><span>{copyStatus || (copying ? 'Copying…' : 'Copy image')}</span>
      </button>
      <button type="button" class="quiet-button" disabled={busy || actionBusy} onclick={() => void perform('save')}>
        <Icon name="folder" size={13} /><span>Save as</span>
      </button>
      <button type="button" class="quiet-button image-result-secondary" aria-label={`Open ${name}`} disabled={busy || actionBusy} onclick={() => void perform('open')}>
        <Icon name="external" size={13} /><span>Open</span>
      </button>
      <button type="button" class="quiet-button image-result-secondary" aria-label={`Reveal ${name} in folder`} disabled={busy || actionBusy} onclick={() => void perform('reveal')}>
        <Icon name="folder" size={13} /><span>Reveal</span>
      </button>
    </div>
    {#if actionError}<p class="image-result-error" role="alert">{actionError}</p>{/if}
    {#if previewError}<p class="image-result-note" title={previewError}>Preview unavailable for this image format.</p>{/if}
  </div>
</article>

<style>
  .image-result { display:grid; grid-template-columns:minmax(96px, 156px) minmax(0, 1fr); align-items:center; gap:13px; min-width:0; padding:10px; border:1px solid var(--line); border-radius:9px; background:var(--surface); }
  .image-result-preview { display:grid; place-items:center; min-width:0; min-height:86px; max-height:156px; overflow:hidden; border:1px solid var(--line); border-radius:7px; background:#fff; color:var(--muted); }
  .image-result-preview img { display:block; width:auto; height:auto; max-width:100%; max-height:154px; object-fit:contain; image-rendering:auto; }
  .image-result-preview > span { padding:8px; font-size:11px; }
  .image-result-spinner { width:17px; height:17px; padding:0 !important; border:2px solid #d8d8d8; border-top-color:#555; border-radius:50%; animation:image-result-spin .75s linear infinite; }
  .image-result-main { display:grid; min-width:0; gap:9px; }
  .image-result-copy { display:grid; min-width:0; gap:4px; }
  .image-result-copy strong { overflow:hidden; color:var(--ink); font-size:12px; font-weight:600; text-overflow:ellipsis; white-space:nowrap; }
  .image-result-copy > span { overflow:hidden; color:var(--muted); font-size:11px; text-overflow:ellipsis; white-space:nowrap; }
  .image-result-actions { display:flex; align-items:center; flex-wrap:wrap; gap:5px; }
  .image-result-actions :global(.quiet-button) { min-height:28px; padding:0 7px; }
  .image-result-actions :global(.quiet-button span) { font-size:11px; }
  .image-result-error, .image-result-note { margin:0; overflow-wrap:anywhere; color:var(--muted); font-size:11px; line-height:1.4; }
  .image-result-error { color:var(--red); }
  @keyframes image-result-spin { to { transform:rotate(360deg); } }
  @media (max-width: 460px) {
    .image-result { grid-template-columns:90px minmax(0, 1fr); gap:9px; padding:8px; }
  }
</style>
