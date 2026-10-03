// Timer and stopwatch state lives at module level so a running countdown keeps
// going while the user switches tools. Times come from Date.now(), so ticks
// that arrive late never drift the result.

export const timer = $state({
  durationMs: 5 * 60_000,
  remainingMs: 5 * 60_000,
  endsAt: 0,
  running: false,
  finished: false,
});

export const stopwatch = $state({
  startedAt: 0,
  carriedMs: 0,
  elapsedMs: 0,
  running: false,
  laps: [] as number[],
});

let ticker: ReturnType<typeof setInterval> | undefined;

function tick(): void {
  const now = Date.now();
  if (timer.running) {
    timer.remainingMs = Math.max(0, timer.endsAt - now);
    if (timer.remainingMs === 0) {
      timer.running = false;
      timer.finished = true;
      alarm();
    }
  }
  if (stopwatch.running) stopwatch.elapsedMs = stopwatch.carriedMs + now - stopwatch.startedAt;
  if (!timer.running && !stopwatch.running) {
    clearInterval(ticker);
    ticker = undefined;
  }
}

function ensureTicking(): void {
  ticker ??= setInterval(tick, 100);
}

function alarm(): void {
  try {
    const context = new AudioContext();
    for (let beep = 0; beep < 3; beep += 1) {
      const oscillator = context.createOscillator();
      const gain = context.createGain();
      oscillator.frequency.value = 880;
      gain.gain.setValueAtTime(0.0001, context.currentTime + beep * 0.45);
      gain.gain.exponentialRampToValueAtTime(0.3, context.currentTime + beep * 0.45 + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, context.currentTime + beep * 0.45 + 0.3);
      oscillator.connect(gain).connect(context.destination);
      oscillator.start(context.currentTime + beep * 0.45);
      oscillator.stop(context.currentTime + beep * 0.45 + 0.32);
    }
    setTimeout(() => void context.close(), 2000);
  } catch {
    // Audio may be unavailable; the finished state is still shown.
  }
}

export function setTimer(ms: number): void {
  timer.durationMs = ms;
  timer.remainingMs = ms;
  timer.running = false;
  timer.finished = false;
}

export function startTimer(): void {
  if (timer.remainingMs <= 0) timer.remainingMs = timer.durationMs;
  if (timer.remainingMs <= 0) return;
  timer.finished = false;
  timer.endsAt = Date.now() + timer.remainingMs;
  timer.running = true;
  ensureTicking();
}

export function pauseTimer(): void {
  tick();
  timer.running = false;
}

export function resetTimer(): void {
  setTimer(timer.durationMs);
}

export function startStopwatch(): void {
  stopwatch.startedAt = Date.now();
  stopwatch.running = true;
  ensureTicking();
}

export function pauseStopwatch(): void {
  tick();
  stopwatch.carriedMs = stopwatch.elapsedMs;
  stopwatch.running = false;
}

export function lapStopwatch(): void {
  tick();
  stopwatch.laps = [stopwatch.elapsedMs, ...stopwatch.laps].slice(0, 99);
}

export function resetStopwatch(): void {
  stopwatch.running = false;
  stopwatch.carriedMs = 0;
  stopwatch.elapsedMs = 0;
  stopwatch.laps = [];
}

export function formatClock(ms: number, tenths = false): string {
  const total = Math.floor(ms / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  const clock = `${hours ? `${hours}:` : ''}${String(minutes).padStart(hours ? 2 : 1, '0')}:${String(seconds).padStart(2, '0')}`;
  return tenths ? `${clock}.${Math.floor((ms % 1000) / 100)}` : clock;
}
