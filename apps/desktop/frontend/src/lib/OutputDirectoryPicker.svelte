<script lang="ts">
  import Icon from './Icon.svelte';
  import { chooseOutputDirectory, revokeOutputDirectory } from './arcade';
  import type { SelectedDirectory } from './contracts';

  let {
    label,
    value,
    disabled = false,
    onSelect,
  }: {
    label: string;
    value: string;
    disabled?: boolean;
    onSelect: (token: string, name: string) => void;
  } = $props();

  let directoryName = $state('');
  let busy = $state(false);
  let error = $state('');
  let previousValue = '';

  $effect(() => {
    if (previousValue && previousValue !== value) void revokeOutputDirectory(previousValue).catch(() => {});
    previousValue = value;
    if (!value) directoryName = '';
  });

  async function choose(): Promise<void> {
    if (busy || disabled) return;
    busy = true;
    error = '';
    try {
      const selected: SelectedDirectory | null = await chooseOutputDirectory();
      if (selected) {
        directoryName = selected.name;
        onSelect(selected.token, selected.name);
      }
    } catch (cause) {
      error = typeof cause === 'string' ? cause : cause instanceof Error ? cause.message : 'The selected folder is unavailable.';
    } finally {
      busy = false;
    }
  }

  function clear(): void {
    directoryName = '';
    error = '';
    onSelect('', '');
  }
</script>

<div class="directory-picker" role="group" aria-label={label} aria-busy={busy}>
  {#if value}
    <div class="directory-selected"><Icon name="folder" size={16} /><span title={directoryName}>{directoryName || 'Selected folder'}</span><button type="button" class="icon-button" aria-label={`Clear ${label}`} title="Use Arcade Box results" disabled={disabled || busy} onclick={clear}><Icon name="close" size={13} /></button></div>
  {:else}
    <button type="button" class="directory-choose-button" disabled={disabled || busy} onclick={() => void choose()}><Icon name="folder" size={15} /><span>{busy ? 'Opening…' : 'Choose folder'}</span><Icon name="arrow" size={13} /></button>
  {/if}
  <small>Optional · leave empty to use Arcade Box results.</small>
  {#if error}<span class="field-error" role="alert">{error}</span>{/if}
</div>
