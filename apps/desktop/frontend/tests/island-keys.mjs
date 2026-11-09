// Behaviour lock for the Island's keys (docs/v1/audit/box.md, box-real.md). A scenario named "KNOWN BUG <id>: ..."
// pins a bug that is not fixed yet: it must FAIL (reported XFAIL); once it passes (XPASS) the fix has landed and the
// marker has to go (docs/v1/DECISIONS.md D3). Runs the visual fixture in Chromium: node tests/island-keys.mjs
import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const port = process.env.KEYS_PORT ?? '5232';
const base = `http://127.0.0.1:${port}/tests/visual.html`;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const assert = (ok, message) => { if (!ok) throw new Error(message); };
const RESULT = { outputs: [{ kind: 'text', mime: 'text/plain', value: '42' }], summary: 'Done', warnings: [] };

const commands = (page, name) => page.evaluate((name) => window.__arcadeFixture.commands.filter((c) => c.command === name).length, name);
const searchFocused = (page) => page.evaluate(() => document.activeElement?.getAttribute('aria-label') === 'Search tools');
const footer = (page) => page.locator('.island-footer').innerText().then((text) => text.replace(/\s+/g, ' ').trim());

async function openTool(page, query = 'convert') {
  await page.keyboard.type(query);
  await page.waitForSelector('.search-panel .overlay-row');
  await sleep(150);
  await page.keyboard.press('Enter');
  await page.waitForSelector('.tool-view');
}

const scenarios = {
  async 'KNOWN BUG BR-0: Escape in a tool returns to the search'(page) {
    await openTool(page);
    await page.keyboard.press('Escape');
    await sleep(200);
    assert(await commands(page, 'hide_island') === 0, 'Escape hid the Island');
    assert(await page.locator('.tool-view').count() === 0, 'the tool is still open');
    assert(await searchFocused(page), 'the search does not have focus');
  },
  async 'KNOWN BUG BR-0: a running job stays reachable from the search'(page) {
    await page.evaluate((result) => {
      const fixture = window.__arcadeFixture;
      fixture.result = result;
      fixture.jobStatus = 'running';
    }, RESULT);
    await openTool(page, 'calculator');
    await page.locator('.tool-view textarea, .tool-view input[type="text"]').first().fill('6*7').catch(() => {});
    await page.keyboard.press('Control+Enter');
    await sleep(300);
    await page.evaluate(() => { window.__arcadeFixture.jobs = [{ id: 'visual-job', toolId: 'arcade.convert.calculator', status: 'running', progress: 0.4, result: null }]; });
    await page.keyboard.press('Escape');
    await sleep(300);
    assert(await commands(page, 'hide_island') === 0, 'Escape hid the Island');
    const indicator = page.locator('.island-shell [data-running-jobs]');
    assert(await indicator.count() === 1, 'no running-job indicator in the search');
    assert(/Jobs/i.test(await indicator.innerText()), 'the indicator does not link to Jobs');
  },
  async 'Escape in a search with text clears it, then closes the empty search'(page) {
    await page.keyboard.type('convert');
    await page.waitForSelector('.search-panel');
    await page.keyboard.press('Escape');
    await sleep(150);
    assert(await page.inputValue('input[aria-label="Search tools"]') === '', 'the search kept its text');
    assert(await commands(page, 'hide_island') === 0, 'the first Escape hid the Island');
    await page.keyboard.press('Escape');
    await sleep(150);
    assert(await commands(page, 'hide_island') === 1, 'Escape in the empty search did not close');
  },
  async 'KNOWN BUG BR-5: the footer says what Escape does'(page) {
    assert(/Esc Close/.test(await footer(page)), `empty search: ${await footer(page)}`);
    await page.keyboard.type('convert');
    await page.waitForSelector('.search-panel');
    assert(/Esc Clear/.test(await footer(page)), `search with text: ${await footer(page)}`);
    await page.keyboard.press('Enter');
    await page.waitForSelector('.tool-view');
    assert(/Esc Back/.test(await footer(page)), `tool: ${await footer(page)}`);
  },
  async 'KNOWN BUG BR-6: the footer, the run button and Settings agree on how to run a tool'(page) {
    await openTool(page);
    const text = await footer(page);
    assert(/(Ctrl|⌘) ?↵ Run/.test(text), `tool footer: ${text}`);
    await page.goto(`${base}?surface=dashboard`);
    await page.waitForSelector('.dashboard-sidebar');
    await page.getByRole('button', { name: /^Settings$/ }).click();
    await sleep(200);
    const settings = (await page.locator('.shell-main').innerText()).replace(/\s+/g, ' ');
    assert(/Run the tool (Ctrl|⌘) ?(\+ ?)?(Enter|↵)/.test(settings), `Settings: ${settings.match(/Run the tool.{0,24}/)?.[0]}`);
  },
  async 'Enter opens the highlighted search result'(page) {
    await page.keyboard.type('convert');
    await page.waitForSelector('.search-panel .overlay-row');
    await sleep(150);
    await page.keyboard.press('ArrowDown');
    const name = await page.locator('.search-panel .overlay-row[aria-selected="true"] .row-title .ellipsis').innerText();
    await page.keyboard.press('Enter');
    await page.waitForSelector('.tool-view');
    const opened = await page.locator('.tool-view h1').innerText();
    assert(opened === name, `opened ${opened}, highlighted ${name}`);
  },
  async 'KNOWN BUG BR-11: a row appearing under a resting pointer does not take the highlight'(page) {
    await page.keyboard.type('convert');
    await page.waitForSelector('.search-panel .overlay-row');
    await sleep(150);
    // WebKitGTK sends mouseenter when a row is laid out under a pointer that has not moved (box-real.md BR-11).
    await page.locator('.search-panel .overlay-row').nth(2).dispatchEvent('mouseenter');
    await sleep(100);
    const selected = await page.locator('.search-panel .overlay-row[aria-selected="true"]').getAttribute('id');
    const first = await page.locator('.search-panel .overlay-row').first().getAttribute('id');
    assert(selected === first, `the highlight moved to ${selected}`);
  },
  async 'A pointer that really moves takes the highlight'(page) {
    await page.keyboard.type('convert');
    await page.waitForSelector('.search-panel .overlay-row');
    await sleep(150);
    const row = page.locator('.search-panel .overlay-row').nth(2);
    const box = await row.boundingBox();
    await page.mouse.move(box.x + 20, box.y + box.height / 2, { steps: 4 });
    await sleep(100);
    assert(await row.getAttribute('aria-selected') === 'true', 'the hovered row is not highlighted');
  },
  async 'KNOWN BUG BR-12: a re-show opens a fresh search with focus'(page) {
    await openTool(page);
    await page.evaluate(async () => {
      const { emit } = window.__arcadeFixture;
      await emit('arcade://island-hiding');
      await emit('arcade://island-hidden');
      await emit('arcade://island-shown');
    });
    await sleep(300);
    assert(await page.locator('.tool-view').count() === 0, 'the old tool came back');
    assert(await page.inputValue('input[aria-label="Search tools"]') === '', 'the old query came back');
    assert(await searchFocused(page), 'the search does not have focus');
  },
};

const vite = spawn(process.execPath, ['node_modules/vite/bin/vite.js', '--port', port, '--strictPort', '--host', '127.0.0.1'], { cwd: root, stdio: 'ignore' });
const results = [];
try {
  for (let i = 0; i < 80; i++) { try { await fetch(base); break; } catch { await sleep(250); } }
  const browser = await chromium.launch(process.env.PLAYWRIGHT_CHROMIUM ? { executablePath: process.env.PLAYWRIGHT_CHROMIUM } : {});
  try {
    for (const [name, run] of Object.entries(scenarios)) {
      const page = await browser.newPage({ viewport: { width: 740, height: 740 } });
      const errors = [];
      page.on('pageerror', (error) => errors.push(error.message));
      try {
        await page.goto(base);
        await page.waitForSelector('.island-shell');
        await page.waitForFunction(() => document.activeElement?.getAttribute('aria-label') === 'Search tools', null, { timeout: 5000 });
        await run(page);
        results.push([name, errors.length ? 'PAGE-ERRORS' : 'PASS', errors.join('; ')]);
      } catch (error) {
        results.push([name, 'FAIL', error.message.split('\n')[0]]);
      } finally {
        await page.close();
      }
    }
  } finally {
    await browser.close();
  }
} finally {
  vite.kill();
}
for (const result of results) {
  if (result[0].startsWith('KNOWN BUG') && result[1] !== 'PAGE-ERRORS') result[1] = result[1] === 'FAIL' ? 'XFAIL' : 'XPASS';
}
for (const [name, status, detail] of results) console.log(`${status} ${name}${detail ? `: ${detail}` : ''}`);
process.exit(results.some(([, status]) => status !== 'PASS' && status !== 'XFAIL') ? 1 : 0);
