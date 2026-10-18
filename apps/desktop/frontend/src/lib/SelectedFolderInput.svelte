<script lang="ts">
  import Icon from './Icon.svelte';
  import { revokeInputFolder, selectInputFolder } from './arcade';
  import type { SelectedDirectory } from './contracts';

  let {
    values,
    maxItems = 1,
    selectionGeneration,
    disabled = false,
    onSelect,
    onRemove,
  }: {
    values: SelectedDirectory[];
    maxItems?: number;
    selectionGeneration: number;
    disabled?: boolean;
    onSelect: (folder: SelectedDirectory) => void;
    onRemove: (token: string) => void;
  } = $props();

  const multiple = $derived(maxItems > 1);

  let busy = $state(false);
  let error = $state('');

  async function choose(): Promise<void> {
    if (busy || disabled) return;
    const generation = selectionGeneration;
    const selectFolder = onSelect;
    busy = true;
    error = '';
    try {
      const folder = await selectInputFolder();
      if (!folder) return;
      if (generation !== selectionGeneration) {
        await revokeInputFolder(folder.token).catch(() => {});
        return;
      }
      selectFolder(folder);
    } catch (cause) {
      error = typeof cause === 'string'
        ? cause
        : cause instanceof Error
          ? cause.message
          : 'The selected folder could not be opened.';
    } finally {
      busy = false;
    }
  }
</script>

<div class="directory-picker input-folder-picker" role="group" aria-label={multiple ? 'Selected input folders' : 'Selected input folder'} aria-busy={busy}>
  {#each values as folder (folder.token)}
    <div class="directory-selected">
      <Icon name="folder" size={16} />
      <span title={folder.name}>{folder.name}</span>
      <button type="button" class="icon-button" aria-label={`Remove folder ${folder.name}`} title="Remove selected folder" disabled={disabled || busy} onclick={() => onRemove(folder.token)}>
        <Icon name="close" size={13} />
      </button>
    </div>
  {/each}
  {#if values.length === 0 || (multiple && values.length < maxItems)}
    <button type="button" class="directory-choose-button" disabled={disabled || busy} onclick={() => void choose()}>
      <Icon name="folder" size={15} />
      <span>{busy ? 'Opening…' : values.length ? 'Add another folder' : 'Choose folder'}</span>
      <Icon name="arrow" size={13} />
    </button>
  {/if}
  <small>{multiple ? 'Read access is limited to these folders for this action.' : 'Read access is limited to this folder for this action.'}</small>
  {#if error}<span class="field-error" role="alert">{error}</span>{/if}
</div>
