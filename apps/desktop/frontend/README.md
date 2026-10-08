# Arcade Box desktop frontend

Svelte 5 renders the Island, dashboard, typed tool forms, previews, job progress,
pipelines, plugin management, Engines and Connected apps. Rust owns execution,
grants, providers, persistence and file outputs.

## Development

Run `npm ci`, `npm run check` and `npm run build` here. `npm run dev` serves
`127.0.0.1:5173` for Tauri development. From the repository root,
`scripts/run-desktop.sh` builds the embedded frontend and runs the installed local
build; this is a user launch, not an isolated test. Use the ecosystem runner for
GUI verification as described in [Arcade Link](../../../docs/arcade-link.md).
Outside Tauri the frontend reports the missing backend instead of returning fake
results.

## Backend boundary

`src/lib/arcade.ts` contains the command wrappers and `src/lib/contracts.ts` the
serialized contracts. `App.svelte` routes selected text, files, folders and screen
inputs through the real forms and scoped backend commands. Media bytes are not
sent through the frontend as large inline payloads. Invalid/unavailable actions
are rejected by the backend even if a cached UI entry is stale.

File pickers, drag/drop, image/video/audio previews and typed results are
implemented. Progress and cancellation follow jobs; the one-second recorder
status timer exists only while recording. Idle screens do not poll providers.
See the [documentation index](../../../docs/STATUS.md) for runtime limits and the
[tool catalog](../../../docs/tool-catalog.md) for per-tool behavior.
