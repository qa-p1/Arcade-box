<script lang="ts">
  import type { Snippet } from 'svelte';
  import { clock } from './media-format';

  type Handle = 'start' | 'end' | 'point';

  let {
    duration,
    mode,
    start = 0,
    end = 0,
    point = 0,
    step = 0.1,
    disabled = false,
    invert = false,
    regions = [],
    label = 'Timeline',
    background,
    onSeek,
    onMove,
  }: {
    duration: number;
    mode: 'range' | 'point';
    start?: number;
    end?: number;
    point?: number;
    /** Seconds per arrow-key step; Shift steps one second. */
    step?: number;
    disabled?: boolean;
    /** The selection is what gets removed, so shade it instead of the rest. */
    invert?: boolean;
    /** Highlighted spans such as detected silence, in seconds. */
    regions?: Array<[number, number]>;
    label?: string;
    background?: Snippet;
    onSeek: (handle: Handle, seconds: number) => void;
    /** Moves the whole range to a new start; dragging inside it is off without this. */
    onMove?: (start: number) => void;
  } = $props();

  let track: HTMLDivElement | undefined = $state();
  let dragging: Handle | 'move' | null = $state(null);
  let moveOffset = 0;

  function percent(seconds: number): string {
    return duration > 0 ? `${Math.max(0, Math.min(100, (seconds / duration) * 100))}%` : '0%';
  }

  function timeAt(event: PointerEvent): number {
    const rect = track!.getBoundingClientRect();
    return Math.max(0, Math.min(duration, ((event.clientX - rect.left) / rect.width) * duration));
  }

  function begin(event: PointerEvent, handle?: Handle): void {
    if (disabled || !track || duration <= 0 || event.button !== 0) return;
    event.preventDefault();
    const time = timeAt(event);
    let chosen: Handle | 'move';
    if (handle) chosen = handle;
    else if (mode === 'point') chosen = 'point';
    else {
      // Inside the range and clear of both handles moves it; elsewhere grabs the nearer handle.
      const slack = (10 / track.getBoundingClientRect().width) * duration;
      const inside = time > start + slack && time < end - slack;
      chosen = inside && onMove ? 'move' : Math.abs(time - start) <= Math.abs(time - end) ? 'start' : 'end';
    }
    dragging = chosen;
    moveOffset = time - start;
    track.setPointerCapture(event.pointerId);
    if (!handle && chosen !== 'move') onSeek(chosen, time);
  }

  function move(event: PointerEvent): void {
    if (!dragging) return;
    const time = timeAt(event);
    if (dragging === 'move') {
      const length = end - start;
      onMove?.(Math.max(0, Math.min(duration - length, time - moveOffset)));
    } else {
      onSeek(dragging, time);
    }
  }

  function finish(event: PointerEvent): void {
    if (!dragging) return;
    dragging = null;
    track?.releasePointerCapture(event.pointerId);
  }

  function nudge(event: KeyboardEvent, handle: Handle): void {
    const amount = event.shiftKey ? 1 : step;
    const current = handle === 'point' ? point : handle === 'start' ? start : end;
    let next: number | null = null;
    if (event.key === 'ArrowLeft' || event.key === 'ArrowDown') next = current - amount;
    else if (event.key === 'ArrowRight' || event.key === 'ArrowUp') next = current + amount;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = duration;
    if (next === null) return;
    event.preventDefault();
    onSeek(handle, Math.max(0, Math.min(duration, next)));
  }
</script>

<div
  class="timeline"
  class:dragging={dragging !== null}
  class:moving={dragging === 'move'}
  bind:this={track}
  role="group"
  aria-label={label}
  onpointerdown={(event) => begin(event)}
  onpointermove={move}
  onpointerup={finish}
  onpointercancel={finish}
>
  <div class="timeline-background" aria-hidden="true">{@render background?.()}</div>
  {#each regions as [from, to], index (index)}
    <div class="region" style:left={percent(from)} style:width={`calc(${percent(to)} - ${percent(from)})`} aria-hidden="true"></div>
  {/each}
  {#if mode === 'range'}
    {#if invert}
      <div class="shade removed" style:left={percent(start)} style:width={`calc(${percent(end)} - ${percent(start)})`}></div>
    {:else}
      <div class="shade" style:left="0" style:width={percent(start)}></div>
      <div class="shade" style:left={percent(end)} style:right="0"></div>
    {/if}
    <div class="selection" class:removed={invert} class:movable={Boolean(onMove)} style:left={percent(start)} style:width={`calc(${percent(end)} - ${percent(start)})`}></div>
    <button type="button" class="handle" style:left={percent(start)} role="slider" aria-label="Start" aria-valuemin={0} aria-valuemax={duration} aria-valuenow={start} aria-valuetext={clock(start)} {disabled}
      onpointerdown={(event) => { event.stopPropagation(); begin(event, 'start'); }} onkeydown={(event) => nudge(event, 'start')}></button>
    <button type="button" class="handle" style:left={percent(end)} role="slider" aria-label="End" aria-valuemin={0} aria-valuemax={duration} aria-valuenow={end} aria-valuetext={clock(end)} {disabled}
      onpointerdown={(event) => { event.stopPropagation(); begin(event, 'end'); }} onkeydown={(event) => nudge(event, 'end')}></button>
  {:else}
    <button type="button" class="handle playhead" style:left={percent(point)} role="slider" aria-label="Position" aria-valuemin={0} aria-valuemax={duration} aria-valuenow={point} aria-valuetext={clock(point)} {disabled}
      onpointerdown={(event) => { event.stopPropagation(); begin(event, 'point'); }} onkeydown={(event) => nudge(event, 'point')}></button>
  {/if}
</div>

<style>
  .timeline { position: relative; height: 52px; border-radius: 9px; background: var(--field-bg); cursor: pointer; touch-action: none; user-select: none; }
  .timeline-background { position: absolute; inset: 0; display: flex; overflow: hidden; border-radius: inherit; }
  .region { position: absolute; top: 0; bottom: 0; background: var(--amber-wash); box-shadow: inset 0 -2px 0 color-mix(in srgb, var(--amber) 70%, transparent); pointer-events: none; }
  .shade { position: absolute; top: 0; bottom: 0; background: rgba(0, 0, 0, .55); pointer-events: none; }
  .shade.removed { background: color-mix(in srgb, var(--red) 28%, rgba(0, 0, 0, .5)); }
  .selection { position: absolute; top: 0; bottom: 0; box-shadow: inset 0 0 0 2px var(--primary); border-radius: 4px; pointer-events: none; }
  .selection.removed { box-shadow: inset 0 0 0 2px var(--red); }
  .handle { position: absolute; top: -4px; bottom: -4px; width: 14px; margin-left: -7px; padding: 0; border: 0; border-radius: 6px; background: var(--primary); box-shadow: 0 1px 4px rgba(0, 0, 0, .4); cursor: ew-resize; touch-action: none; }
  .handle::after { position: absolute; top: 50%; left: 50%; width: 2px; height: 16px; border-radius: 1px; background: var(--primary-ink); content: ''; opacity: .45; transform: translate(-50%, -50%); }
  .handle.playhead { width: 4px; margin-left: -2px; border-radius: 2px; }
  .handle.playhead::after { display: none; }
  .timeline.dragging, .timeline.dragging .handle { cursor: grabbing; }
  .timeline.moving { cursor: grabbing; }
</style>
