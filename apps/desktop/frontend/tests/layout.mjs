// Layout check (BOX.md acceptance): no element is wider than its box in the
// Island (compact, search, tool, first run) or on any dashboard page, at
// 320×116, 740×740, 720×520 and 1600×1000, in light and dark. Scrolling
// containers, ellipsised text and screen-reader-only text are allowed to be.
// Run after `npm run build`-able sources: node tests/layout.mjs
// PLAYWRIGHT_CHROMIUM may name a Chromium binary to use.
import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const port = process.env.LAYOUT_PORT ?? '5231';
const vite = spawn(process.execPath, ['node_modules/vite/bin/vite.js', '--port', port, '--strictPort', '--host', '127.0.0.1'], { cwd: root, stdio: 'ignore' });
const base = `http://127.0.0.1:${port}/tests/visual.html`;
const failures = [];

function overflowing() {
  return [...document.querySelectorAll('body *')].filter((element) => {
    const style = getComputedStyle(element);
    if (style.display === 'none' || style.visibility === 'hidden' || element.closest('.visually-hidden, .sr-only')) return false;
    if (['auto', 'scroll'].includes(style.overflowX) || style.textOverflow === 'ellipsis') return false;
    if (style.webkitLineClamp && style.webkitLineClamp !== 'none') return false;
    return element.clientWidth > 0 && element.scrollWidth > element.clientWidth + 1;
  }).slice(0, 5).map((element) => `${element.tagName.toLowerCase()}.${[...element.classList].join('.')} (${element.scrollWidth} > ${element.clientWidth})`);
}

async function check(page, label, theme) {
  await page.evaluate((value) => { document.documentElement.dataset.theme = value; }, theme);
  await page.waitForTimeout(250);
  const found = await page.evaluate(overflowing);
  if (found.length) failures.push(`${label} (${theme}): ${found.join(', ')}`);
}

try {
  for (let i = 0; i < 80; i++) { try { await fetch(base); break; } catch { await new Promise((resolve) => setTimeout(resolve, 250)); } }
  const browser = await chromium.launch(process.env.PLAYWRIGHT_CHROMIUM ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM } : {});
  try {
    for (const theme of ['light', 'dark']) {
      for (const [width, height] of [[320, 116], [740, 740]]) {
        const size = `${width}×${height}`;
        const page = await browser.newPage({ viewport: { width, height } });
        page.on('pageerror', (error) => failures.push(`Island ${size}: ${error.message}`));
        await page.goto(base);
        await page.waitForSelector('.island-shell');
        await check(page, `Island compact ${size}`, theme);
        await page.keyboard.type('convert');
        await page.waitForSelector('.search-panel');
        await check(page, `Island search ${size}`, theme);
        await page.keyboard.press('Enter');
        await page.waitForSelector('.tool-view');
        await check(page, `Island tool ${size}`, theme);
        await page.goto(`${base}?onboarding=1`);
        await page.waitForSelector('.onboarding');
        await check(page, `Island first run ${size}`, theme);
        await page.close();
      }
      for (const [width, height] of [[720, 520], [1100, 760], [1600, 1000]]) {
        const size = `${width}×${height}`;
        const page = await browser.newPage({ viewport: { width, height } });
        page.on('pageerror', (error) => failures.push(`Dashboard ${size}: ${error.message}`));
        await page.goto(`${base}?surface=dashboard`);
        await page.waitForSelector('.dashboard-sidebar');
        for (const name of ['Tools', 'Favorites', 'Recent', 'Jobs', 'Pipelines', 'Plugins', 'Engines', 'Settings']) {
          await page.locator('.dashboard-sidebar .tab-list').first().getByRole('button', { name: new RegExp(`^${name}`) }).click();
          await page.waitForTimeout(150);
          await check(page, `Dashboard ${name} ${size}`, theme);
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
  console.error(`Layout check failed:\n  ${failures.join('\n  ')}`);
  process.exit(1);
}
console.log('Layout check: nothing overflows in the Island or the dashboard');
