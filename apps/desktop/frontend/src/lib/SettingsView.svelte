<script lang="ts">
  import Icon from './Icon.svelte';
  import ConnectedApps from './ConnectedApps.svelte';
  import type { ShortcutStatus } from './contracts';
  import { setShortcut } from './arcade';

  type Theme = 'system' | 'light' | 'dark';
  let {
    theme,
    shortcut,
    onThemeChange,
    onShortcutChange,
  }: {
    theme: Theme;
    shortcut: ShortcutStatus | null;
    onThemeChange: (theme: Theme) => Promise<void> | void;
    onShortcutChange: (status: ShortcutStatus) => void;
  } = $props();

  let shortcutValue = $state('');
  let savingShortcut = $state(false);
  let shortcutError = $state('');
  let localStatus = $state<ShortcutStatus | null>(null);
  let themeMessage = $state('');
  let themeError = $state('');

  $effect(() => {
    shortcutValue = shortcut?.triggerDescription || shortcut?.trigger_description || '';
    localStatus = shortcut;
  });

  function errorText(error: unknown): string {
    if (typeof error === 'string') return error;
    if (error instanceof Error) return error.message;
    return 'The shortcut could not be updated.';
  }

  async function updateShortcut(): Promise<void> {
    if (!shortcutValue.trim() || savingShortcut) return;
    savingShortcut = true;
    shortcutError = '';
    try {
      const status = await setShortcut(shortcutValue.trim());
      localStatus = status;
      onShortcutChange(status);
    } catch (error) {
      shortcutError = errorText(error);
    } finally {
      savingShortcut = false;
    }
  }

  async function chooseTheme(value: string): Promise<void> {
    if (value !== 'system' && value !== 'light' && value !== 'dark') return;
    themeError = '';
    themeMessage = '';
    try {
      await onThemeChange(value);
      themeMessage = `Theme set to ${value}.`;
    } catch (error) {
      themeError = errorText(error);
    }
  }

  function shortcutStateLabel(status: ShortcutStatus | null): string {
    if (!status) return 'Status unavailable';
    const state = status.state.toLowerCase();
    if (state === 'registered') return 'Registered';
    if (state === 'unsupported' || state === 'unavailable' || state === 'hyprland_binding_unavailable') return 'Unavailable on this desktop';
    if (state === 'starting' || state === 'checking' || state === 'updating') return 'Checking shortcut';
    if (state === 'registration_rejected' || state === 'invalid_trigger' || state === 'error') return 'Needs attention';
    return state.replaceAll('_', ' ');
  }
</script>

<section class="settings-view" aria-label="Arcade Box settings">
  <ConnectedApps />
  <section class="settings-card" aria-labelledby="appearance-heading">
    <div class="settings-card-heading"><span class="settings-symbol"><Icon name="spark" size={18} /></span><div><h2 id="appearance-heading">Appearance</h2><p>Choose how Arcade Box follows your desktop.</p></div></div>
    <fieldset class="theme-choice-group">
      <legend>Theme</legend>
      {#each [{ id: 'system', label: 'System', detail: 'Follow your desktop' }, { id: 'light', label: 'Light', detail: 'Use a bright palette' }, { id: 'dark', label: 'Dark', detail: 'Use a dim palette' }] as choice}
        <label class="theme-choice" class:theme-choice-active={theme === choice.id}>
          <input type="radio" name="arcade-theme" value={choice.id} checked={theme === choice.id} onchange={(event) => void chooseTheme((event.currentTarget as HTMLInputElement).value)} />
          <span class="theme-choice-preview" class:preview-light={choice.id === 'light'} class:preview-system={choice.id === 'system'}><i></i><i></i><i></i></span>
          <span class="theme-choice-copy"><strong>{choice.label}</strong><small>{choice.detail}</small></span>
          {#if theme === choice.id}<Icon name="check" size={15} />{/if}
        </label>
      {/each}
    </fieldset>
    {#if themeMessage}<p class="settings-feedback" role="status">{themeMessage}</p>{/if}
    {#if themeError}<p class="field-error" role="alert">{themeError}</p>{/if}
  </section>

  <section class="settings-card" aria-labelledby="shortcut-heading">
    <div class="settings-card-heading"><span class="settings-symbol"><Icon name="command" size={18} /></span><div><h2 id="shortcut-heading">Global shortcut</h2><p>Bring up the Arcade Island from any application.</p></div></div>
    <form class="settings-shortcut-form" onsubmit={(event) => { event.preventDefault(); void updateShortcut(); }}>
      <label for="settings-shortcut">Shortcut</label>
      <div class="settings-shortcut-row"><input id="settings-shortcut" bind:value={shortcutValue} maxlength="80" placeholder="Ctrl+Alt+Space" spellcheck="false" /><button class="pipeline-primary-button" type="submit" disabled={savingShortcut || !shortcutValue.trim()}><Icon name="check" size={14} />{savingShortcut ? 'Checking…' : 'Save shortcut'}</button></div>
      {#if localStatus}<div class="settings-shortcut-status" class:shortcut-good={localStatus.state.toLowerCase() === 'registered'} class:shortcut-bad={!['registered', 'starting', 'checking', 'updating'].includes(localStatus.state.toLowerCase())}><span class="runtime-dot" class:offline={localStatus.state.toLowerCase() !== 'registered'}></span><strong>{shortcutStateLabel(localStatus)}</strong><span>{localStatus.message}</span></div>{/if}
      {#if shortcutError}<p class="field-error" role="alert">{shortcutError}</p>{/if}
      {#if localStatus?.backend.toLowerCase().includes('portal') || localStatus?.backend.toLowerCase().includes('wayland')}<p class="settings-note">On Wayland, global shortcuts depend on your desktop's GlobalShortcuts portal. A portal permission prompt may appear when you save.</p>{/if}
      <p class="settings-note">If another application owns this shortcut, choose a different key combination. Arcade Box reports whether the desktop registered it.</p>
    </form>
  </section>

  <section class="settings-card settings-privacy-card" aria-labelledby="privacy-heading">
    <div class="settings-card-heading"><span class="settings-symbol"><Icon name="lock" size={18} /></span><div><h2 id="privacy-heading">Privacy</h2><p>Arcade Box keeps ordinary tool processing on this device when a local provider is available.</p></div></div>
    <div class="settings-facts"><div><strong>Clipboard context</strong><span>Read only when Arcade Box is invoked; clipboard history is a separate opt-in feature.</span></div><div><strong>Tool labels</strong><span>Each action identifies whether it is LOCAL, NETWORK, or CLOUD.</span></div><div><strong>Diagnostics</strong><span>Tool inputs and secrets are not included in normal diagnostics.</span></div></div>
  </section>
</section>
