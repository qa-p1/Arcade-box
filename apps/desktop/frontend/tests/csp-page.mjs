// Serves the visual fixture under the built pages' Content-Security-Policy (csp.mjs, BR-18) and reports every
// violation Chromium logs, so the layout and key tests also prove the Island and dashboard work under it.
import { CONTENT_SECURITY_POLICY } from '../csp.mjs';

export async function underPolicy(page, report) {
  await page.route((url) => url.pathname.endsWith('/tests/visual.html'), async (route) => {
    const response = await route.fetch();
    await route.fulfill({ response, headers: { ...response.headers(), 'content-security-policy': CONTENT_SECURITY_POLICY } });
  });
  page.on('console', (message) => {
    if (message.type() === 'error' && /Content Security Policy/i.test(message.text())) report(`CSP: ${message.text()}`);
  });
}
