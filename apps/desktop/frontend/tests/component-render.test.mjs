import assert from 'node:assert/strict';
import { after, before, test } from 'node:test';
import { readFile } from 'node:fs/promises';
import { createServer } from 'vite';
import { fileURLToPath } from 'node:url';
import { defaultUiValues, standardUiOptionsProblem } from '../src/lib/standard-ui.ts';

let server;
let Form, Result, Diff, render;
const catalog = JSON.parse(await readFile(new URL('../../../../catalog/tools.json', import.meta.url), 'utf8'));

before(async () => {
  server = await createServer({ root: fileURLToPath(new URL('..', import.meta.url)), server: { middlewareMode: true }, appType: 'custom' });
  render = (await server.ssrLoadModule('svelte/server')).render;
  Form = (await server.ssrLoadModule('/src/lib/StandardToolForm.svelte')).default;
  Result = (await server.ssrLoadModule('/src/lib/ToolOutputView.svelte')).default;
  Diff = (await server.ssrLoadModule('/src/lib/DiffInput.svelte')).default;
});
after(async () => { await server?.close(); });

test('every catalog form renders with valid defaults; password controls stay masked', () => {
  let count = 0;
  for (const tool of catalog.tools) {
    if (!tool.ui) continue;
    const values = defaultUiValues(tool.ui);
    assert.equal(standardUiOptionsProblem(tool.ui, values), '', tool.id);
    const html = render(Form, { props: { ui: tool.ui, values, idPrefix: tool.id, onValueChange() {} } }).body;
    assert.ok(!html.includes('unsupported form'), tool.id);
    for (const control of (tool.ui.controls ?? []).filter((item) => item.type === 'password')) {
      if (html.includes(`id="${tool.id}-${control.key}"`)) {
        assert.match(html, new RegExp(`id="${tool.id}-${control.key}"[^>]*type="password"`), tool.id);
      }
    }
    count++;
  }
  assert.ok(count > 100);
});

test('structured reports page their rows and escape content', () => {
  const value = JSON.stringify(Array.from({ length: 5000 }, (_, index) => ({ row: index, text: index === 0 ? '<script>unsafe</script>' : 'content' })));
  const html = render(Result, { props: { output: { kind: 'text', mime: 'structured/report', value }, toolId: 'test.report' } }).body;
  assert.equal((html.match(/<tr\b/g) ?? []).length, 41); // header + first page
  assert.ok(html.includes('of 5000'));
  assert.ok(!html.includes('<script>unsafe</script>'));
  assert.ok(html.includes('Copy result'));
});

test('text previews are bounded but preserve formatting', () => {
  const html = render(Result, { props: { output: { kind: 'text', mime: 'text/plain', value: 'x'.repeat(100_000) }, toolId: 'test.text' } }).body;
  assert.ok(html.length < 30_000);
  assert.ok(html.includes('Copy includes the complete result'));
});

test('diff and merge offer separate labeled inputs', () => {
  const props = { left: 'one', right: 'two', base: 'base', merge: false };
  assert.equal((render(Diff, { props }).body.match(/<textarea/g) ?? []).length, 2);
  assert.equal((render(Diff, { props: { ...props, merge: true } }).body.match(/<textarea/g) ?? []).length, 3);
});
