<script lang="ts">
  import Icon from './Icon.svelte';
  import { revokeInputFolder, selectInputFolder } from './arcade';
  import type { SelectedDirectory } from './contracts';

  let {
    value,
    selectionGeneration,
    disabled = false,
    onSelect,
    onClear,
  }: {
    value: SelectedDirectory | null;
    selectionGeneration: number;
    disabled?: boolean;
    onSelect: (folder: SelectedDirectory) => void;
    onClear: () => void;
  } = $props();

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

<div class="directory-picker input-folder-picker" role="group" aria-label="Selected input folder" aria-busy={busy}>
  {#if value}
    <div class="directory-selected">
      <Icon name="folder" size={16} />
      <span title={value.name}>{value.name}</span>
      <button type="button" class="icon-button" aria-label={`Remove folder ${value.name}`} title="Remove selected folder" disabled={disabled || busy} onclick={onClear}>
        <Icon name="close" size={13} />
      </button>
    </div>
  {:else}
    <button type="button" class="directory-choose-button" disabled={disabled || busy} onclick={() => void choose()}>
      <Icon name="folder" size={15} />
      <span>{busy ? 'Opening…' : 'Choose folder'}</span>
      <Icon name="arrow" size={13} />
    </button>
  {/if}
  <small>Read access is limited to this folder for this action.</small>
  {#if error}<span class="field-error" role="alert">{error}</span>{/if}
</div>
