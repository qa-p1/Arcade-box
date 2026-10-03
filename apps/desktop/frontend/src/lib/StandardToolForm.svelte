<script lang="ts">
  import type { StandardToolUi, UiChoice, UiControl } from './contracts';
  import { isUiControlVisible, type UiValues } from './standard-ui';
  import OutputDirectoryPicker from './OutputDirectoryPicker.svelte';

  let {
    ui,
    values,
    idPrefix,
    allowDirectory = true,
    disabled = false,
    capabilities = null,
    onValueChange,
  }: {
    ui: StandardToolUi;
    values: UiValues;
    idPrefix: string;
    allowDirectory?: boolean;
    disabled?: boolean;
    /** Capabilities of the compatible installed providers; null while unknown. */
    capabilities?: ReadonlySet<string> | null;
    onValueChange: (key: string, value: string) => void;
  } = $props();

  const visibleControls = $derived(ui.version === 1 ? (ui.controls ?? []).filter((control) => isUiControlVisible(control, values)) : []);
  const regularControls = $derived(visibleControls.filter((control) => !control.advanced));
  const advancedControls = $derived(visibleControls.filter((control) => control.advanced));

  function controlId(control: UiControl): string {
    return `${idPrefix}-${control.key}`;
  }

  function choiceAvailable(choice: UiChoice): boolean {
    return !capabilities || !choice.requires?.length || choice.requires.every((capability) => capabilities.has(capability));
  }

  function isMultilineControl(control: UiControl): boolean {
    if (control.type !== 'text') return false;
    const copy = `${control.label} ${control.help ?? ''}`;
    return /\bper line\b/i.test(copy) || (control.key === 'regions' && /\bjson\b/i.test(copy));
  }
</script>

{#snippet controlField(control: UiControl)}
  {@const id = controlId(control)}
  {@const helpId = control.help ? `${id}-help` : undefined}
  <div class="standard-control-row" data-control-type={control.type}>
    <div class="standard-control-label">
      {#if control.type === 'directory'}<span>{control.label}</span>{:else}<label for={id}>{control.label}</label>{/if}
      {#if control.help}<span id={helpId}>{control.help}</span>{/if}
    </div>
    {#if control.type === 'select'}
      <select id={id} value={values[control.key] ?? ''} aria-describedby={helpId} disabled={disabled} onchange={(event) => onValueChange(control.key, (event.currentTarget as HTMLSelectElement).value)}>
        {#each control.choices ?? [] as choice (choice.value)}{@const available = choiceAvailable(choice)}<option value={choice.value} disabled={!available && values[control.key] !== choice.value}>{choice.label}{available ? '' : ' — not installed'}</option>{/each}
      </select>
    {:else if control.type === 'number'}
      <input id={id} type="number" value={values[control.key] ?? ''} min={control.minimum} max={control.maximum} step={control.step} placeholder={control.placeholder ?? ''} aria-describedby={helpId} disabled={disabled} oninput={(event) => onValueChange(control.key, (event.currentTarget as HTMLInputElement).value)} />
    {:else if control.type === 'toggle'}
      <label class="standard-toggle" for={id} aria-label={control.label}>
        <input id={id} type="checkbox" checked={values[control.key] === 'true'} aria-describedby={helpId} disabled={disabled} onchange={(event) => onValueChange(control.key, (event.currentTarget as HTMLInputElement).checked ? 'true' : 'false')} />
        <span aria-hidden="true"></span>
      </label>
    {:else if control.type === 'directory'}
      {#if allowDirectory}
        <OutputDirectoryPicker label={control.label} value={values[control.key] ?? ''} disabled={disabled} onSelect={(token) => onValueChange(control.key, token)} />
      {:else}
        <p class="standard-directory-note">Choose a save folder when you run this action. Pipeline results stay in Arcade Box.</p>
      {/if}
    {:else if isMultilineControl(control)}
      <textarea
        id={id}
        class="standard-control-textarea"
        rows={control.key === 'regions' ? 4 : 3}
        value={values[control.key] ?? ''}
        placeholder={control.placeholder ?? ''}
        aria-describedby={helpId}
        spellcheck="false"
        disabled={disabled}
        oninput={(event) => onValueChange(control.key, (event.currentTarget as HTMLTextAreaElement).value)}
      ></textarea>
    {:else}
      <input
        id={id}
        type={control.type === 'password' ? 'password' : 'text'}
        value={values[control.key] ?? ''}
        placeholder={control.placeholder ?? ''}
        aria-describedby={helpId}
        autocomplete={control.type === 'password' ? 'off' : undefined}
        autocapitalize="off"
        spellcheck="false"
        disabled={disabled}
        oninput={(event) => onValueChange(control.key, (event.currentTarget as HTMLInputElement).value)}
      />
    {/if}
  </div>
{/snippet}

{#if ui.version !== 1}
  <div class="standard-ui-error" role="alert">This tool uses an unsupported form version ({ui.version}). Update Arcade Box to use it.</div>
{:else}
  <div class="standard-tool-form" aria-label="Tool options">
    {#each regularControls as control (control.key)}
      {@render controlField(control)}
    {/each}

    {#if advancedControls.length}
      <details class="standard-advanced-options">
        <summary>More options <span>{advancedControls.length}</span></summary>
        <div class="standard-advanced-controls">
          {#each advancedControls as control (control.key)}
            {@render controlField(control)}
          {/each}
        </div>
      </details>
    {/if}
  </div>
{/if}
