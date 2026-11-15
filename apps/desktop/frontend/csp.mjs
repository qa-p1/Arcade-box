// The Content-Security-Policy of the Island and dashboard windows (BR-18), in one place for the build
// (vite.config.ts injects it into index.html and dashboard.html) and the browser tests (tests/layout.mjs runs
// every surface under it and fails on any violation). Nothing remote: scripts only from Box's own files, images
// and media from Box's files and the data:/blob: URLs its previews make, connections only to Tauri's IPC.
// Inline styles stay allowed: Svelte's style attributes set progress bars, ruler and colour-sampler positions.
// frame-ancestors is left out because a <meta> policy ignores it; pin.html keeps its own stricter policy.
export const CONTENT_SECURITY_POLICY = [
  "default-src 'none'",
  "script-src 'self'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: blob:",
  "media-src 'self' data: blob:",
  "font-src 'self' data:",
  "connect-src 'self' ipc: http://ipc.localhost data: blob:",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
].join('; ');

/** Pages that get the policy; pin.html carries its own. */
const PAGES = /(^|\/)(index|dashboard)\.html$/;

/** Vite plugin: the policy as the first <meta> of the built Island and dashboard pages. */
export function contentSecurityPolicy() {
  return {
    name: 'arcade-box-content-security-policy',
    apply: 'build',
    transformIndexHtml(html, context) {
      if (!PAGES.test(context.path) || /http-equiv=["']Content-Security-Policy/i.test(html)) return html;
      return [{ tag: 'meta', attrs: { 'http-equiv': 'Content-Security-Policy', content: CONTENT_SECURITY_POLICY }, injectTo: 'head-prepend' }];
    },
  };
}
