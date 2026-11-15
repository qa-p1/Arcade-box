// README and user-guide screenshots (docs/user/images), light and dark, from the visual fixture at 2x:
//   node tests/screenshots.mjs        (PLAYWRIGHT_CHROMIUM may name a Chromium binary)
// Retake them after a visible UI change, and look at every image before committing it.
import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const images = fileURLToPath(new URL('../../../../docs/user/images/', import.meta.url));
const port = process.env.SHOTS_PORT ?? '5234';
const base = `http://127.0.0.1:${port}/tests/visual.html`;
const RESULT = { status: 'success', outputs: [{ kind: 'text', mime: 'text/plain', value: 'QUARTERLY REPORT FOR THE ARCADE TEAM' }], summary: 'Done', warnings: [] };

async function island(page, theme) {
  await page.goto(base);
  await page.waitForSelector('.island-shell');
  await page.evaluate((value) => { document.documentElement.dataset.theme = value; }, theme);
  await page.locator('input[aria-label="Search tools"]').focus();
}

const shots = {
  async 'island-results'(page, theme) {
    await island(page, theme);
    await page.keyboard.type('compress');
    await page.waitForSelector('.search-panel .overlay-row');
    await page.waitForTimeout(300);
  },
  async 'tool-run'(page, theme) {
    await island(page, theme);
    await page.evaluate((result) => { window.__arcadeFixture.result = result; }, RESULT);
    await page.keyboard.type('change case');
    await page.waitForSelector('.search-panel .overlay-row');
    await page.waitForTimeout(150);
    await page.keyboard.press('Enter');
    await page.waitForSelector('.tool-view');
    await page.locator('.tool-view textarea').first().fill('quarterly report for the arcade team');
    const select = page.locator('.tool-view select').first();
    if (await select.count()) await select.selectOption({ label: 'UPPERCASE' }).catch(() => {});
    await page.keyboard.press('Control+Enter');
    await page.waitForSelector('.tool-view :text("QUARTERLY REPORT")');
    await page.waitForTimeout(300);
  },
  async 'dashboard-tools'(page, theme) {
    await page.goto(`${base}?surface=dashboard`);
    await page.waitForSelector('.dashboard-sidebar');
    await page.evaluate((value) => { document.documentElement.dataset.theme = value; }, theme);
    await page.waitForTimeout(300);
  },
};
const sizes = { 'island-results': [740, 372], 'tool-run': [740, 554], 'dashboard-tools': [1100, 720] };

const vite = spawn(process.execPath, ['node_modules/vite/bin/vite.js', '--port', port, '--strictPort', '--host', '127.0.0.1'], { cwd: root, stdio: 'ignore' });
try {
  for (let i = 0; i < 80; i++) { try { await fetch(base); break; } catch { await new Promise((resolve) => setTimeout(resolve, 250)); } }
  const browser = await chromium.launch(process.env.PLAYWRIGHT_CHROMIUM ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM } : {});
  try {
    for (const [name, take] of Object.entries(shots)) {
      for (const theme of ['light', 'dark']) {
        const [width, height] = sizes[name];
        const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 2, colorScheme: theme, reducedMotion: 'reduce' });
        await take(page, theme);
        await page.mouse.move(0, height - 1);
        await page.screenshot({ path: `${images}${name}-${theme}.png` });
        console.log(`${name}-${theme}.png`);
        await page.close();
      }
    }
  } finally {
    await browser.close();
  }
} finally {
  vite.kill();
}
