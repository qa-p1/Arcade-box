<script lang="ts">
  // First run (STANDARD §5.1): the kit hero inside the Island, the shortcut
  // set with the same recorder as Settings, then "Browse tools". "Skip" is
  // always there, and Settings › General can show this again.
  import Icon from '../lib/Icon.svelte';
  import ShortcutRecorder from '../lib/ShortcutRecorder.svelte';
  import boxGlyph from '../design/glyphs/arcade.box.svg';
  import { setShortcut, shortcutOwner } from '../lib/arcade';
  import { useBox } from '../box.svelte';

  const box = useBox();
  const shortcut = $derived(box.shortcutInfo?.triggerDescription || box.shortcutInfo?.trigger_description || '');
  const registered = $derived(box.shortcutInfo?.state.toLowerCase() === 'registered');

  async function apply(chord: string): Promise<void> {
    const status = await setShortcut(chord);
    box.acceptShortcutStatus(status);
    if (!['registered', 'starting', 'checking', 'updating'].includes(status.state.toLowerCase())) throw new Error(status.message);
  }
  async function owners(chord: string): Promise<string[]> {
    const owner = await shortcutOwner(chord);
    return owner ? [owner] : [];
  }
</script>
<section class="onboarding" aria-labelledby="onboarding-title" aria-describedby="onboarding-description">
  <div class="hero">
    <span class="glyph-tile lg brand-tile" aria-hidden="true"><img src={boxGlyph} alt="" width="26" height="26" /></span>
    <h1 id="onboarding-title">Welcome to Arcade Box</h1>
    <p id="onboarding-description">Press your shortcut, type what you need and press Enter. Box closes back to the app you were in.</p>
  </div>
  <ol class="list steps">
    <li class="row step" bind:this={box.shortcutField}>
      <span class="step-number" class:done={registered}>{#if registered}<Icon name="check" size={13} />{:else}1{/if}</span>
      <div class="row-main">
        <div class="row-title">Set a shortcut</div>
        <div class="row-sub">{box.shortcutInfo && !registered ? box.shortcutInfo.message : 'Opens the Island from any app.'}</div>
      </div>
      <ShortcutRecorder value={shortcut} label="Shortcut for the Island" defaultValue={box.defaultShortcutForPlatform()} conflictsFor={owners} onApply={apply} />
    </li>
    <li class="row step">
      <span class="step-number">2</span>
      <div class="row-main">
        <div class="row-title">Browse tools</div>
        <div class="row-sub">See all {box.runnableTools.length} tools in the dashboard.</div>
      </div>
      <button type="button" class="btn sm" onclick={() => void box.finishOnboarding().then(() => box.toggleDashboard())}>Browse tools</button>
    </li>
  </ol>
  <div class="dialog-actions">
    <button type="button" class="btn ghost" onclick={() => void box.finishOnboarding()}>Skip</button>
    <button type="button" class="btn primary" onclick={() => void box.finishOnboarding()}>Start using Box<Icon name="arrow" size={14} /></button>
  </div>
</section>

<style>
  .onboarding { padding: var(--sp-32) var(--sp-24) var(--sp-20); }
  .brand-tile { width: 52px; height: 52px; background: var(--text); box-shadow: none; }
  .brand-tile img { width: 26px; height: 26px; filter: invert(1); }
  :global(html[data-theme="dark"]) .brand-tile img { filter: none; }
  .steps { margin: var(--sp-24) 0 0; padding: 0; list-style: none; }
  .step { flex-wrap: wrap; }
  .step-number { display: grid; flex: none; width: 24px; height: 24px; place-items: center; border-radius: var(--radius-pill); background: var(--sunken); box-shadow: inset 0 0 0 1px var(--line); color: var(--muted); font-size: var(--fs-small); font-weight: var(--fw-semibold); }
  .step-number.done { background: var(--ok-soft); box-shadow: none; color: var(--ok); }
  .step .row-main { flex: 1 1 200px; }
</style>
