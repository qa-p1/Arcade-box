// BR-18: the Island and dashboard pages are built with a strict Content-Security-Policy.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { CONTENT_SECURITY_POLICY, contentSecurityPolicy } from '../csp.mjs';
import config from '../vite.config.ts';

const plugin = () => (config.plugins ?? []).flat().find((p) => p?.name === 'arcade-box-content-security-policy');
const directives = Object.fromEntries(CONTENT_SECURITY_POLICY.split(';').map((d) => d.trim().split(/\s+/)).map(([name, ...values]) => [name, values]));

test('the build injects the policy into the Island and dashboard pages only', () => {
  assert.ok(plugin(), 'vite.config.ts uses the CSP plugin');
  const { transformIndexHtml } = contentSecurityPolicy();
  for (const path of ['/index.html', '/dashboard.html']) {
    const tags = transformIndexHtml('<html><head></head></html>', { path });
    assert.deepEqual(tags, [{ tag: 'meta', attrs: { 'http-equiv': 'Content-Security-Policy', content: CONTENT_SECURITY_POLICY }, injectTo: 'head-prepend' }]);
  }
  const pin = '<html><head><meta http-equiv="Content-Security-Policy" content="default-src \'none\'"></head></html>';
  assert.equal(transformIndexHtml(pin, { path: '/pin.html' }), pin);
});

test('the policy allows nothing remote and no script outside Box\'s own files', () => {
  assert.deepEqual(directives['default-src'], ["'none'"]);
  assert.deepEqual(directives['script-src'], ["'self'"]);
  assert.deepEqual(directives['object-src'], ["'none'"]);
  assert.deepEqual(directives['base-uri'], ["'none'"]);
  for (const [name, values] of Object.entries(directives)) {
    for (const value of values) {
      assert.ok(!/^(https?:|\*|'unsafe-eval'|'wasm-unsafe-eval')/.test(value) || value === 'http://ipc.localhost', `${name} ${value}`);
    }
  }
});
