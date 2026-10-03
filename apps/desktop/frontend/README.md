# Arcade Box desktop frontend

This Svelte 5 frontend is the visual surface for the Tauri desktop application. It contains the compact Arcade Island, keyboard driven tool search, tool execution controls, context suggestions, and the catalog dashboard.

## Local development

Run `npm install` and `npm run dev` from this directory. The frontend listens on `127.0.0.1:5173`, matching `apps/desktop/src-tauri/tauri.conf.json`. Outside Tauri it shows the backend connection state instead of inventing a catalog or returning simulated results.

## Backend boundary

All desktop work goes through `src/lib/arcade.ts`. Its commands use the serialized types in `src/lib/contracts.ts`, which mirror `crates/arcade-contract`:

- `list_tools`
- `search_tools({ query })`
- `run_tool({ request: { toolId, inputs, options } })`
- `detect_context`
- `run_context_action({ toolId })`
- `hide_island`

Search results with `status !== 'implemented'` appear under catalog discovery and cannot be invoked with Enter. A selected non-text tool explains which input type needs a file-picker/artifact connection; it does not fake a text run.

## First-party controls

The UI has explicit options for the shared text executors:

- Case conversion: upper, lower, title, sentence, camel, Pascal, snake, kebab.
- Whitespace cleaning: trim lines, collapse, remove blank lines, normalize endings, tabs to spaces.
- Structured data: JSON, YAML, TOML, XML with supported pretty, compact, or validate modes.

Each run submits a typed text value and selected options to the Rust runtime. Context suggestions call `run_context_action` so clipboard-backed JSON can be formatted without copying its content through frontend code.
