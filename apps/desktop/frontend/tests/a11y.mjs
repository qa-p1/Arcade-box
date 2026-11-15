// Accessibility check (catalog dev/v1 README "Accessibility basics"; docs/v1/DECISIONS.md D21). In Chromium, on the
// visual fixture, in light and dark:
//   - axe-core's WCAG 2.1 A and AA rules (contrast, names, roles, labels) on the Island (empty, search, tool, first
//     run) and on every dashboard page;
//   - every control Tab reaches on a dashboard page shows a visible focus indicator;
//   - with reduced motion requested, nothing animates or transitions for longer than 10 ms.
// Text at 125% is covered by tests/layout.mjs. Run: node tests/a11y.mjs (PLAYWRIGHT_CHROMIUM may name a Chromium).
import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const axeSource = readFileSync(createRequire(import.meta.url).resolve('axe-core/axe.min.js'), 'utf8');
const port = process.env.A11Y_PORT ?? '5233';
const base = `http://127.0.0.1:${port}/tests/visual.html`;
const PAGES = ['Tools', 'Favorites', 'Recent', 'Jobs', 'Pipelines', 'Plugins', 'Engines', 'Settings'];
const failures = [];

async function audit(page, label) {
  if (!await page.evaluate(() => 'axe' in window)) await page.evaluate(axeSource);
  const violations = await page.evaluate(async () => {
    const result = await window.axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'] } });
    return result.violations.map((v) => `${v.id} (${v.impact}): ${v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ')}${v.nodes.length > 3 ? ` +${v.nodes.length - 3}` : ''}`);
  });
  for (const violation of violations) failures.push(`${label}: ${violation}`);
}

// A focused control must look different from the same control unfocused: an outline, a ring (box-shadow), a border
// or a background change, on the control or on the field around it (a search box rings its frame, :focus-within).
// Compared by blurring and refocusing the same element, so a style that only shows on hover does not count.
async function focusRing(page, label) {
  await page.evaluate(() => document.activeElement?.blur());
  const seen = new Set();
  for (let i = 0; i < 40; i++) {
    await page.keyboard.press('Tab');
    const found = await page.evaluate(() => {
      const el = document.activeElement;
      if (!el || el === document.body) return null;
      const look = () => [el, el.parentElement, el.parentElement?.parentElement].filter(Boolean).map((node) => {
        const s = getComputedStyle(node);
        return [s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) > 0 ? `${s.outlineStyle} ${s.outlineWidth} ${s.outlineColor}` : '', s.boxShadow, s.borderColor, s.backgroundColor].join('|');
      }).join(' / ');
      const id = `${el.tagName.toLowerCase()}.${[...el.classList].join('.')}[${el.getAttribute('aria-label') ?? el.textContent?.trim().slice(0, 24) ?? ''}]`;
      const focused = look();
      el.blur();
      const resting = look();
      el.focus();
      return { id, visible: focused !== resting };
    });
    if (!found || seen.has(found.id)) continue;
    seen.add(found.id);
    if (!found.visible) failures.push(`${label}: no visible focus on ${found.id}`);
  }
}

async function motion(page, label) {
  const moving = await page.evaluate(() => [...document.querySelectorAll('body *')].filter((el) => {
    const s = getComputedStyle(el);
    const longest = (value) => Math.max(...value.split(',').map((t) => parseFloat(t) * (t.trim().endsWith('ms') ? 1 : 1000)));
    const animated = s.animationName !== 'none' && longest(s.animationDuration) > 10;
    return animated || longest(s.transitionDuration) > 10;
  }).slice(0, 5).map((el) => `${el.tagName.toLowerCase()}.${[...el.classList].join('.')}`));
  if (moving.length) failures.push(`${label} (reduced motion): still animates ${moving.join(', ')}`);
}

const vite = spawn(process.execPath, ['node_modules/vite/bin/vite.js', '--port', port, '--strictPort', '--host', '127.0.0.1'], { cwd: root, stdio: 'ignore' });
try {
  for (let i = 0; i < 80; i++) { try { await fetch(base); break; } catch { await new Promise((resolve) => setTimeout(resolve, 250)); } }
  const browser = await chromium.launch(process.env.PLAYWRIGHT_CHROMIUM ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM } : {});
  try {
    for (const theme of ['light', 'dark']) {
      for (const reducedMotion of ['no-preference', 'reduce']) {
        const page = await browser.newPage({ viewport: { width: 740, height: 740 }, reducedMotion, colorScheme: theme });
        page.on('pageerror', (error) => failures.push(`Island ${theme}: ${error.message}`));
        const setTheme = () => page.evaluate((value) => { document.documentElement.dataset.theme = value; }, theme);
        const check = async (label) => {
          await setTheme();
          await page.waitForTimeout(250);
          if (reducedMotion === 'reduce') await motion(page, `${label} ${theme}`);
          else await audit(page, `${label} ${theme}`);
        };
        await page.goto(base);
        await page.waitForSelector('.island-shell');
        await check('Island empty');
        await page.locator('input[aria-label="Search tools"]').focus();
        await page.keyboard.type('convert');
        await page.waitForSelector('.search-panel .overlay-row');
        await check('Island search');
        await page.keyboard.press('Enter');
        await page.waitForSelector('.tool-view');
        await check('Island tool');
        await page.goto(`${base}?onboarding=1`);
        await page.waitForSelector('.onboarding');
        await check('Island first run');
        await page.goto(`${base}?surface=dashboard`);
        await page.waitForSelector('.dashboard-sidebar');
        for (const name of PAGES) {
          await page.locator('.dashboard-sidebar .tab-list').first().getByRole('button', { name: new RegExp(`^${name}`) }).click();
          await page.waitForTimeout(150);
          await check(`Dashboard ${name}`);
          if (reducedMotion === 'no-preference') await focusRing(page, `Dashboard ${name} ${theme}`);
        }
        await page.close();
      }
    }
  } finally {
    await browser.close();
  }
} finally {
  vite.kill();
}
if (failures.length) {
  console.error(`Accessibility check failed (${failures.length}):\n  ${[...new Set(failures)].join('\n  ')}`);
  process.exit(1);
}
console.log('Accessibility check: WCAG 2.1 A/AA (axe-core) on the Island and every dashboard page, visible focus on every Tab stop, no motion when reduced, in both themes');
