<script lang="ts">
  // Colour conversion result: a swatch, every format with copy buttons, and
  // the WCAG contrast grades against the chosen second colour.
  let { value, copy }: { value: unknown; mime?: string; copy: (text: string) => void } = $props();
  const report = $derived((value && typeof value === 'object' ? value : {}) as Record<string, unknown>);
  const contrast = $derived((report.contrast ?? {}) as Record<string, string>);
  const formats = $derived((['hex', 'rgb', 'hsl', 'hsv', 'cmyk'] as const).filter((key) => typeof report[key] === 'string').map((key) => [key, report[key] as string]));
  const grades = [['normalTextAA', 'Normal text AA'], ['normalTextAAA', 'Normal text AAA'], ['largeTextAA', 'Large text AA'], ['largeTextAAA', 'Large text AAA'], ['uiComponents', 'Icons & UI']] as const;
  const hex = $derived(typeof report.hex === 'string' && /^#[0-9a-f]{6}$/i.test(report.hex) ? report.hex : '#000000');
  const against = $derived(/^#[0-9a-f]{6}$/i.test(contrast.against ?? '') ? contrast.against : '#ffffff');
</script>

<div class="color-report">
  <div class="swatches">
    <div class="swatch" style:background={hex}><span style:color={report.bestTextColor === 'black' ? '#000' : '#fff'}>{report.name ?? hex}</span></div>
    <div class="sample" style:background={against} style:color={hex}><strong>Aa</strong><span>Text on {against}</span></div>
  </div>
  <div class="formats">{#each formats as [key, text] (key)}<button type="button" title="Copy {text}" onclick={() => copy(text)}><span>{key.toUpperCase()}</span><code>{text}</code></button>{/each}</div>
  <p class="ratio">Contrast <strong>{contrast.ratio ?? '—'}</strong></p>
  <ul class="grades">{#each grades as [key, label] (key)}<li class:pass={contrast[key] === 'Pass'}>{contrast[key] === 'Pass' ? '✓' : '✕'} {label}</li>{/each}</ul>
</div>

<style>
  .color-report { display: grid; gap: 12px; }
  .swatches { display: grid; grid-template-columns: 1.4fr 1fr; gap: 8px; }
  .swatch, .sample { display: flex; align-items: flex-end; min-height: 84px; padding: 10px 12px; border-radius: 10px; border: 1px solid var(--line); }
  .swatch span { font: 600 13px var(--mono); }
  .sample { flex-direction: column; align-items: flex-start; justify-content: center; gap: 2px; }
  .sample strong { font-size: 26px; line-height: 1; }
  .sample span { font-size: 12px; }
  .formats { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 6px; }
  .formats button { display: grid; gap: 2px; padding: 8px 10px; text-align: left; border: 1px solid var(--line); border-radius: 8px; background: var(--surface-1); color: var(--ink); cursor: pointer; }
  .formats button:hover { background: var(--surface-2); }
  .formats span { font-size: 10.5px; color: var(--ink-3); letter-spacing: .04em; }
  .formats code { font-size: 12px; }
  .ratio { margin: 0; font-size: 13px; color: var(--ink-2); }
  .ratio strong { color: var(--ink); font-size: 15px; }
  .grades { display: flex; flex-wrap: wrap; gap: 6px; margin: 0; padding: 0; list-style: none; }
  .grades li { padding: 4px 9px; border-radius: 999px; font-size: 12px; color: var(--red); background: var(--red-wash); }
  .grades li.pass { color: var(--mint); background: var(--mint-wash); }
</style>
