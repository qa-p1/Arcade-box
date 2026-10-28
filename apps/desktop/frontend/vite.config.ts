import { defineConfig } from 'vite';
import { svelte, vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';

const frontendRoot = fileURLToPath(new URL('.', import.meta.url));
// Box's one version lives in the workspace Cargo.toml.
const workspace = readFileSync(new URL('../../../Cargo.toml', import.meta.url), 'utf8');
const version = /^\[workspace\.package\][^[]*?^version = "([^"]+)"/ms.exec(workspace)?.[1];
if (!version) throw new Error('No [workspace.package] version in Cargo.toml');

export default defineConfig({
  plugins: [svelte({ preprocess: vitePreprocess() })],
  clearScreen: false,
  define: { __BOX_VERSION__: JSON.stringify(version) },
  server: {
    strictPort: true,
    port: 1420,
  },
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    // Safari 15 is the oldest target esbuild can lower Svelte 5's destructuring
    // output to. It also matches Arcade Box's maintained macOS baseline.
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari15',
    minify: !process.env.TAURI_ENV_DEBUG ? 'esbuild' : false,
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
    rollupOptions: {
      input: {
        main: resolve(frontendRoot, 'index.html'),
        dashboard: resolve(frontendRoot, 'dashboard.html'),
        pin: resolve(frontendRoot, 'pin.html'),
      },
    },
  },
});
