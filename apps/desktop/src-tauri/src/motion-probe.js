(() => {
  if (!window.__motionProbe) {
    const original = window.__TAURI_INTERNALS__.invoke;
    let calls = 0;
    window.__TAURI_INTERNALS__.invoke = function(command, ...args) {
      if (command === 'set_island_input_region') calls++;
      return original.call(this, command, ...args);
    };
    window.__motionProbe = (label) => {
      const begin = performance.now(), before = calls, times = [];
      let previous;
      const frame = (time) => {
        if (previous !== undefined) times.push(time - previous);
        previous = time;
        if (time - begin < 700) return requestAnimationFrame(frame);
        const sorted = [...times].sort((a,b)=>a-b);
        original('profile_motion', {sample: JSON.stringify({label, frames: times.length,
          avgMs: +(times.reduce((a,b)=>a+b,0) / times.length).toFixed(2),
          p95Ms: +sorted[Math.floor(sorted.length*.95)]?.toFixed(2),
          maxMs: +Math.max(...times).toFixed(2),
          over25Ms: times.filter(t=>t>25).length, inputRegionCalls: calls-before})});
      };
      requestAnimationFrame(frame);
    };
    document.addEventListener('keydown', e => window.__motionProbe(e.key === 'Enter' ? 'open-tool' : 'key'), {capture:true});
  }
  window.__motionProbe('show');
})();