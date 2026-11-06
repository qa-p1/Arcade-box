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

async function checkScroll(page, label, selector, required = false) {
  const pane = page.locator(selector);
  const metrics = await pane.evaluate((el) => {
    el.scrollTop = 0;
    const r = el.getBoundingClientRect();
    return { range: el.scrollHeight - el.clientHeight, top: r.top, bottom: r.bottom, viewport: innerHeight };
  });
  if (metrics.top < -1 || metrics.bottom > metrics.viewport + 1) {
    failures.push(`${label}: scroll pane extends outside the window`);
    return;
  }
  if (metrics.range <= 2) {
    if (required) failures.push(`${label}: expected a long, scrollable page`);
    return;
  }
  await pane.hover({ position: { x: 15, y: 80 } });
  await page.mouse.wheel(0, 480);
  try {
    await page.waitForFunction((selector) => document.querySelector(selector).scrollTop > 20, selector, { timeout: 1500 });
  } catch {
    failures.push(`${label}: mouse wheel cannot scroll the content`);
  }
  await pane.evaluate((el) => { el.scrollTop = el.scrollHeight; });
  const atEnd = await pane.evaluate((el) => Math.abs(el.scrollHeight - el.clientHeight - el.scrollTop) <= 2);
  if (!atEnd) failures.push(`${label}: bottom of content is unreachable`);
  await pane.evaluate((el) => { el.scrollTop = 0; });
  if (selector === '.shell-main') {
    await pane.focus();
    await page.keyboard.press('PageDown');
    try {
      await page.waitForFunction(() => document.querySelector('.shell-main').scrollTop > 20, null, { timeout: 1500 });
    } catch {
      failures.push(`${label}: PageDown cannot scroll the focused content`);
    }
    await page.keyboard.press('Control+End');
    await page.waitForTimeout(200);
    if (!await pane.evaluate((el) => Math.abs(el.scrollHeight - el.clientHeight - el.scrollTop) <= 2)) failures.push(`${label}: keyboard cannot reach the bottom`);
    await pane.evaluate((el) => { el.scrollTop = 0; });
  }
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
          await checkScroll(page, `Dashboard ${name} ${size} ${theme}`, '.shell-main', ['Tools', 'Settings'].includes(name));
        }
        // Search shortcuts must work from pages with no mounted search field.
        await page.keyboard.press('Control+k');
        try {
          await page.waitForFunction(() => document.activeElement?.getAttribute('aria-label') === 'Search tools', null, { timeout: 1500 });
        } catch { failures.push(`Dashboard ${size} ${theme}: search shortcut from Settings did not focus Tools search`); }
        const content = page.locator('.shell-main');
        await content.evaluate((el) => { el.scrollTop = el.scrollHeight; });
        await page.getByRole('button', { name: /^Settings$/, exact: true }).click();
        await page.waitForTimeout(100);
        if (!await content.evaluate((el) => el.scrollTop === 0)) failures.push(`Dashboard ${size} ${theme}: new page retained the previous scroll position`);
        await checkScroll(page, `Sidebar categories ${size} ${theme}`, '.category-list');
        const lastCategory = page.locator('.category-list button').last();
        await lastCategory.focus();
        const categoryVisible = await lastCategory.evaluate((el) => {
          const r = el.getBoundingClientRect(), parent = el.closest('.category-list').getBoundingClientRect();
          return r.top >= parent.top - 1 && r.bottom <= parent.bottom + 1;
        });
        if (!categoryVisible) failures.push(`Sidebar ${size} ${theme}: keyboard focus leaves a category clipped`);
        if (width === 720) {
          await page.evaluate(() => {
            const style = getComputedStyle(document.documentElement);
            for (const name of Array.from(style).filter(name => name.startsWith('--fs-') || name.startsWith('--lh-'))) {
              const value = style.getPropertyValue(name).trim();
              if (value.endsWith('px')) document.documentElement.style.setProperty(name, `${parseFloat(value) * 1.25}px`);
            }
          });
          await check(page, `Dashboard Settings ${size} 125% text`, theme);
          await checkScroll(page, `Dashboard Settings ${size} 125% text ${theme}`, '.shell-main', true);
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
console.log('Layout/UX check: Island and all dashboard pages fit; wheel/keyboard scrolling, page reset, sidebar focus and search shortcut pass in both themes, including 125% text');
