<script lang="ts">
  import Icon from '../lib/Icon.svelte';
  import ShortcutClash from '../lib/ShortcutClash.svelte';
  import { useBox } from '../box.svelte';

  const box = useBox();
</script>
<div class="onboarding-backdrop" role="presentation">
  <dialog bind:this={box.onboardingDialog} class="onboarding-card" aria-modal="true" aria-labelledby="onboarding-title" aria-describedby="onboarding-description" oncancel={(event) => event.preventDefault()}>
    <div class="onboarding-brand"><span class="brand-mark"><Icon name="command" size={19} /></span><span class="brand-name">Arcade <span>Box</span></span><span class="onboarding-step">First run</span></div>
    <h1 id="onboarding-title">Meet Arcade Box.</h1>
    <p id="onboarding-description">Open the Arcade Island, type what you need, and press Enter. It closes back to the app you were using.</p>
    <div class="onboarding-shortcut-block">
      <label for="onboarding-shortcut">Global shortcut</label>
      <div class="onboarding-shortcut-input"><Icon name="command" size={17} /><input bind:this={box.shortcutField} bind:value={box.shortcutInput} id="onboarding-shortcut" autocomplete="off" spellcheck="false" aria-describedby="shortcut-status" /><span class="shortcut-edit-hint">Editable</span></div>
      <ShortcutClash accelerator={box.shortcutInput} />
      <div class="shortcut-status" id="shortcut-status" class:shortcut-good={box.shortcutInfo?.state === 'registered'} class:shortcut-bad={box.shortcutInfo && box.shortcutInfo.state !== 'registered' && box.shortcutInfo.state !== 'starting' && box.shortcutInfo.state !== 'checking' && box.shortcutInfo.state !== 'updating'}><span class="runtime-dot" class:offline={box.shortcutInfo?.state !== 'registered'}></span><span>{box.shortcutInfo?.message || 'A platform default will be tested when you continue.'}</span></div>
      {#if box.shortcutInfo?.backend.toLowerCase().includes('portal')}
        <p class="portal-note">Wayland registers shortcuts through the XDG Desktop Portal. Your desktop may remap or decline a shortcut; if the portal is unavailable, Arcade Box will explain the limitation.</p>
        <small class="shortcut-syntax">Portal format example: CTRL+ALT+space</small>
      {:else}
        <small class="shortcut-syntax">Use a modifier and key, for example Ctrl+Alt+Space.</small>
      {/if}
    </div>
    {#if box.shortcutError}<div class="onboarding-error" role="status">{box.shortcutError}</div>{/if}
    <div class="onboarding-actions"><button class="onboarding-primary" disabled={box.shortcutSaving} onclick={() => void box.applyShortcutAndContinue()}>{box.shortcutSaving ? 'Checking shortcut…' : 'Save shortcut & continue'}<Icon name="arrow" size={15} /></button><button class="onboarding-secondary" onclick={() => void box.finishOnboarding()}>Continue without changing it</button></div>
    <div class="onboarding-flow"><span><kbd>⌘</kbd> invoke</span><Icon name="chevron" size={13} /><span>type</span><Icon name="chevron" size={13} /><span><kbd>↵</kbd> perform</span></div>
  </dialog>
</div>
