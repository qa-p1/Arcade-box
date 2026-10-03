<script lang="ts">
  import Icon from './Icon.svelte';
  import type { IconName } from './icon-names';
  import type { SelectedFile } from './contracts';

  let {
    files,
    label,
    multiple = false,
    sortable = false,
    maxItems,
    disabled = false,
    onAdd,
    onRemove,
    onReorder,
    formatSize,
  }: {
    files: SelectedFile[];
    label: string;
    multiple?: boolean;
    sortable?: boolean;
    maxItems?: number;
    disabled?: boolean;
    onAdd: () => void;
    onRemove: (token: string) => void;
    onReorder: (fromToken: string, toIndex: number) => void;
    formatSize: (size: number) => string;
  } = $props();

  let draggedToken = '';
  const atLimit = $derived(multiple && maxItems !== undefined && files.length >= maxItems);
  let announcement = $state('');

  function iconFor(mime: string): IconName {
    if (mime.startsWith('file/image')) return 'image';
    if (mime.startsWith('file/pdf') || mime.startsWith('file/document')) return 'document';
    if (mime.startsWith('file/video')) return 'video';
    if (mime.startsWith('file/audio')) return 'audio';
    return 'file';
  }

  function move(token: string, delta: number): void {
    const index = files.findIndex((file) => file.token === token);
    const target = index + delta;
    if (index < 0 || target < 0 || target >= files.length) return;
    onReorder(token, target);
    announcement = `${files[index].name} moved to position ${target + 1} of ${files.length}.`;
  }

  function dropAt(token: string, index: number): void {
    const from = files.findIndex((file) => file.token === token);
    if (from < 0 || from === index) return;
    onReorder(token, index);
    announcement = `${files[from].name} moved to position ${index + 1} of ${files.length}.`;
  }

  function fileType(mime: string): string {
    const type = mime.replace(/^file\//, '').replace(/^application\//, '').replace(/^image\//, '');
    return type ? type.toUpperCase() : 'FILE';
  }
</script>

{#if files.length > 0}
  <div class="granted-file-list" role="list" aria-label={sortable ? `${label} in processing order` : `${label} selected`}>
    {#each files as file, index (file.token)}
      <div
        class="granted-file-row"
        class:granted-file-sortable={sortable}
        role="listitem"
        aria-posinset={index + 1}
        aria-setsize={files.length}
        draggable={sortable}
        ondragstart={sortable ? (event) => { draggedToken = file.token; if (event.dataTransfer) { event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setData('text/plain', file.token); } } : undefined}
        ondragover={sortable ? (event) => event.preventDefault() : undefined}
        ondrop={sortable ? (event) => { event.preventDefault(); dropAt(draggedToken || event.dataTransfer?.getData('text/plain') || '', index); draggedToken = ''; } : undefined}
        ondragend={() => (draggedToken = '')}
      >
        {#if sortable}<span class="granted-file-handle" aria-hidden="true">⋮⋮</span><span class="granted-file-index" aria-hidden="true">{String(index + 1).padStart(2, '0')}</span>{/if}
        <span class="selected-file-icon"><Icon name={iconFor(file.mime)} size={17} /></span>
        <span class="selected-file-copy"><strong title={file.name}>{file.name}</strong><small title={file.mime}>{formatSize(file.size)} · {fileType(file.mime)}</small></span>
        <div class="granted-file-actions">
          {#if sortable}
            <button type="button" class="icon-button order-file-button" aria-label={`Move ${file.name} up`} title="Move up" disabled={disabled || index === 0} onclick={() => move(file.token, -1)}><Icon name="arrow" size={14} /></button>
            <button type="button" class="icon-button order-file-button order-down" aria-label={`Move ${file.name} down`} title="Move down" disabled={disabled || index === files.length - 1} onclick={() => move(file.token, 1)}><Icon name="arrow" size={14} /></button>
          {/if}
          <button type="button" class="icon-button remove-file-button" aria-label={`Remove ${file.name}`} title="Remove file" disabled={disabled} onclick={() => onRemove(file.token)}><Icon name="close" size={14} /></button>
        </div>
      </div>
    {/each}
  </div>
  {#if sortable}<span class="sr-only" role="status" aria-live="polite" aria-atomic="true">{announcement}</span>{/if}
{/if}

<button type="button" class="file-picker-button granted-file-picker" disabled={disabled || atLimit} onclick={onAdd}>
  <span class="file-picker-icon"><Icon name={files.length && multiple ? 'plus' : 'file'} size={17} /></span>
  <span><strong>{atLimit ? 'File limit reached' : files.length ? (multiple ? `Add ${label}` : `Replace ${label}`) : `Choose ${label}`}</strong><small>{files.length ? `${files.length} selected${maxItems ? ` · ${atLimit ? `maximum ${maxItems}` : `up to ${maxItems}`}` : ''}` : 'Open file picker'}</small></span>
  <Icon name="arrow" size={15} />
</button>
