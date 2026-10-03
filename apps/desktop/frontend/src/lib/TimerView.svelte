<script lang="ts">
  // Countdown timer and stopwatch. State is shared module state, so both keep
  // running while another tool is open.
  import Icon from './Icon.svelte';
  import { formatClock, lapStopwatch, pauseStopwatch, pauseTimer, resetStopwatch, resetTimer, setTimer, startStopwatch, startTimer, stopwatch, timer } from './timer-state.svelte';

  let mode = $state<'timer' | 'stopwatch'>(stopwatch.running && !timer.running ? 'stopwatch' : 'timer');
  let hours = $state(Math.floor(timer.durationMs / 3_600_000));
  let minutes = $state(Math.floor((timer.durationMs % 3_600_000) / 60_000));
  let seconds = $state(Math.floor((timer.durationMs % 60_000) / 1000));
  const presets = [1, 3, 5, 10, 15, 25, 30, 60];
  const progress = $derived(timer.durationMs ? timer.remainingMs / timer.durationMs : 0);

  function applyFields(): void {
    const clamp = (value: number, max: number) => Math.min(max, Math.max(0, Math.floor(Number(value) || 0)));
    setTimer((clamp(hours, 99) * 3600 + clamp(minutes, 59) * 60 + clamp(seconds, 59)) * 1000);
  }

  function preset(value: number): void {
    hours = Math.floor(value / 60);
    minutes = value % 60;
    seconds = 0;
    setTimer(value * 60_000);
  }
</script>

<div class="timer-view">
  <div class="segmented" role="tablist" aria-label="Timer mode">
    <button type="button" role="tab" aria-selected={mode === 'timer'} class:active={mode === 'timer'} onclick={() => (mode = 'timer')}>Timer</button>
    <button type="button" role="tab" aria-selected={mode === 'stopwatch'} class:active={mode === 'stopwatch'} onclick={() => (mode = 'stopwatch')}>Stopwatch</button>
  </div>

  {#if mode === 'timer'}
    <div class="dial" class:finished={timer.finished} style:--progress={progress}>
      <span class="time" aria-live="polite">{timer.finished ? "Time's up" : formatClock(timer.remainingMs)}</span>
    </div>
    {#if !timer.running}
      <div class="fields">
        <label>Hours<input type="number" min="0" max="99" bind:value={hours} onchange={applyFields} /></label>
        <label>Minutes<input type="number" min="0" max="59" bind:value={minutes} onchange={applyFields} /></label>
        <label>Seconds<input type="number" min="0" max="59" bind:value={seconds} onchange={applyFields} /></label>
      </div>
      <div class="presets">{#each presets as value (value)}<button type="button" onclick={() => preset(value)}>{value < 60 ? `${value} min` : '1 hour'}</button>{/each}</div>
    {/if}
    <div class="actions">
      {#if timer.running}
        <button type="button" class="primary" onclick={pauseTimer}><Icon name="pause" size={14} /><span>Pause</span></button>
      {:else}
        <button type="button" class="primary" disabled={timer.durationMs === 0} onclick={startTimer}><Icon name="play" size={14} /><span>{timer.remainingMs < timer.durationMs && !timer.finished ? 'Resume' : 'Start'}</span></button>
      {/if}
      <button type="button" onclick={resetTimer}><span>Reset</span></button>
    </div>
  {:else}
    <div class="dial stopwatch"><span class="time" aria-live="off">{formatClock(stopwatch.elapsedMs, true)}</span></div>
    <div class="actions">
      {#if stopwatch.running}
        <button type="button" class="primary" onclick={pauseStopwatch}><Icon name="pause" size={14} /><span>Pause</span></button>
        <button type="button" onclick={lapStopwatch}><span>Lap</span></button>
      {:else}
        <button type="button" class="primary" onclick={startStopwatch}><Icon name="play" size={14} /><span>{stopwatch.elapsedMs ? 'Resume' : 'Start'}</span></button>
        <button type="button" disabled={!stopwatch.elapsedMs} onclick={resetStopwatch}><span>Reset</span></button>
      {/if}
    </div>
    {#if stopwatch.laps.length}
      <ol class="laps">
        {#each stopwatch.laps as lap, index (index)}
          <li><span>Lap {stopwatch.laps.length - index}</span><span>+{formatClock(lap - (stopwatch.laps[index + 1] ?? 0), true)}</span><strong>{formatClock(lap, true)}</strong></li>
        {/each}
      </ol>
    {/if}
  {/if}
</div>

<style>
  .timer-view { display: grid; gap: 14px; padding: 14px; justify-items: center; }
  .segmented { display: inline-flex; padding: 3px; border-radius: 9px; background: var(--surface-1); }
  .segmented button { padding: 6px 16px; border: 0; border-radius: 7px; background: transparent; color: var(--ink-2); cursor: pointer; font-size: 13px; }
  .segmented button.active { background: var(--surface-3); color: var(--ink); }
  .dial { --progress: 1; display: grid; place-items: center; width: 190px; height: 190px; border-radius: 50%; background: conic-gradient(var(--ink-2) calc(var(--progress) * 360deg), var(--surface-2) 0); }
  .dial::before { content: ''; grid-area: 1 / 1; width: 176px; height: 176px; border-radius: 50%; background: var(--panel); }
  .dial.stopwatch { background: var(--surface-2); }
  .dial.finished { background: var(--amber); }
  .time { grid-area: 1 / 1; z-index: 1; font: 600 34px/1 var(--mono); color: var(--ink); font-variant-numeric: tabular-nums; }
  .dial.finished .time { font-size: 22px; color: var(--amber); }
  .fields { display: flex; gap: 10px; }
  .fields label { display: grid; gap: 4px; font-size: 11.5px; color: var(--ink-3); }
  .fields input { width: 72px; padding: 7px 9px; border: 1px solid var(--field-line); border-radius: 8px; background: var(--field-bg); color: var(--ink); font: 14px var(--mono); }
  .presets { display: flex; flex-wrap: wrap; justify-content: center; gap: 6px; }
  .presets button, .actions button { display: inline-flex; align-items: center; gap: 6px; padding: 6px 12px; border: 1px solid var(--line); border-radius: 8px; background: var(--surface-1); color: var(--ink-2); cursor: pointer; font-size: 12.5px; }
  .presets button:hover, .actions button:hover:not(:disabled) { background: var(--surface-2); color: var(--ink); }
  .actions { display: flex; gap: 8px; }
  .actions button { padding: 8px 18px; font-size: 13px; }
  .actions button.primary { background: var(--primary); color: var(--primary-ink); border-color: transparent; }
  .actions button:disabled { opacity: .45; cursor: default; }
  .laps { width: min(100%, 360px); max-height: 220px; overflow: auto; margin: 0; padding: 0; list-style: none; font-size: 13px; }
  .laps li { display: grid; grid-template-columns: 1fr auto auto; gap: 16px; padding: 7px 4px; border-bottom: 1px solid var(--line); color: var(--ink-2); font-variant-numeric: tabular-nums; }
  .laps strong { color: var(--ink); font-weight: 600; }
</style>
