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

  // OCR languages (B9): installed Tesseract packs as a multi-select whose
  // value is Tesseract's "eng+deu" form. Unknown packs fall back to the text field.
  const LANGUAGE_NAMES: Record<string, string> = {
    eng: 'English', deu: 'German', fra: 'French', spa: 'Spanish', ita: 'Italian', por: 'Portuguese', nld: 'Dutch',
    pol: 'Polish', rus: 'Russian', ukr: 'Ukrainian', tur: 'Turkish', jpn: 'Japanese', kor: 'Korean', ara: 'Arabic',
    hin: 'Hindi', chi_sim: 'Chinese (Simplified)', chi_tra: 'Chinese (Traditional)', osd: 'Orientation and script',
  };
  const installedLanguages = $derived(capabilities ? [...capabilities].filter((item) => item.startsWith('ocr:language:')).map((item) => item.slice('ocr:language:'.length)).sort((a, b) => (a === 'eng' ? -1 : b === 'eng' ? 1 : a.localeCompare(b))) : []);
  const isLanguageControl = (control: UiControl) => control.key === 'language' && /\bOCR\b/.test(control.label) && installedLanguages.length > 0;
  const chosenLanguages = (value: string | undefined) => (value ?? '').split('+').map((code) => code.trim()).filter(Boolean);
  function toggleLanguage(control: UiControl, code: string, on: boolean): void {
    const chosen = chosenLanguages(values[control.key]).filter((item) => item !== code);
    const next = on ? [...chosen, code] : chosen;
    onValueChange(control.key, next.join('+'));
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
    {:else if isLanguageControl(control)}
      {@const chosen = chosenLanguages(values[control.key])}
      <fieldset class="language-choices" aria-describedby={helpId} {disabled}>
        <legend class="visually-hidden">{control.label}</legend>
        {#each [...installedLanguages, ...chosen.filter((code) => !installedLanguages.includes(code))] as code (code)}
          {@const installed = installedLanguages.includes(code)}
          <label class="language-choice" class:missing={!installed}>
            <input type="checkbox" checked={chosen.includes(code)} onchange={(event) => toggleLanguage(control, code, event.currentTarget.checked)} />
            <span>{LANGUAGE_NAMES[code] ?? code}{installed ? '' : ' — not installed'}</span>
          </label>
        {/each}
      </fieldset>
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

<style>
  .language-choices { display: flex; flex-wrap: wrap; gap: var(--sp-6); min-width: 0; margin: 0; padding: 0; border: 0; }
  .language-choice { display: inline-flex; align-items: center; gap: var(--sp-6); padding: var(--sp-4) var(--sp-10); border: 1px solid var(--line); border-radius: var(--radius-pill); font-size: var(--fs-small); cursor: pointer; }
  .language-choice:has(input:checked) { border-color: var(--accent); background: var(--accent-soft); }
  .language-choice.missing { color: var(--warn); }
</style>
