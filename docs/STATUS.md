# Arcade Box: status

Verified 2026-10-08 on branch `arcade/link` (version 0.1.0, Arcade Link
`v0.1.0`). This page records what is implemented and how it was checked;
the other documents describe design and behavior.

Box now pins Arcade Link `v0.2.0`, which adds Arcade Shelf and Arcade Find to
the shared app metadata, so Connected apps lists both with a Get link. The
checks below were recorded against `v0.1.0`; the 2026-10-08 figures are not
re-measured for the new pin.

Version 0.1.2 adds **Add to Shelf** to the result action row: output files,
folders and text go to Arcade Shelf's `shelf.add` as references (see
[Arcade Link](arcade-link.md)). It is covered by core unit tests against a
mock Shelf manifest; a run against a real Shelf is not recorded here yet.

## Implemented

- The Island and dashboard (Tauri 2, Svelte 5), the CLI (`arcade-box`, alias
  `arcadebox`) and the shared Rust core. All 109 catalog tools have status
  `implemented`; whether one runs on a given machine depends on its input,
  platform, permissions and optional engines.
- Pipelines (typed DAG, peer stages, effect approval, repair), jobs with
  progress and cancellation, history, favorites, aliases, sandboxed WASM
  plugins run in the `arcade-plugin-worker` sidecar, the Engines page and the
  standard tray menu.
- Engines are found on the system, never bundled or installed silently:
  FFmpeg, libvips, ImageMagick, qpdf, Poppler, Ghostscript, Tesseract, yt-dlp,
  curl, an installed browser for webpage capture, the system voice for text
  to speech. Images to PDF and document to PDF are built in. Tesseract can be
  downloaded per user from Engines (Link's shared `engines` folder).
- Arcade Link: every tool and preset as `box:<tool>[#preset]`, `box.open`,
  `box.pipelines`, `box.pipeline.run`, one-shot mode, and the Lens, Look,
  Clipboard, Wheel, Shelf and Tools integrations listed in [Arcade Link](arcade-link.md).
- Hyprland: window-pin floats and pins the previous window; paste-plain sends
  Ctrl+Shift+V to it.

## Verification

| Check | Result |
|---|---|
| `cargo test -p arcade-core` | 136 passed, 1 ignored (network: Firefox-family capture, run by hand and passing) |
| `cargo test -p arcade-desktop --lib` | 14 passed |
| `npm run check`, `npm run build` (frontend) | 0 errors, 0 warnings; builds |
| `python scripts/license_inventory.py --check` | inventory current |
| CI (Linux, Windows, macOS: tests, frontend, installers) | passing at `ae67811` |
| Arcade Link e2e (`box`, `box-overlap`, `box-pipeline` and cross-app groups) | all passing (74/74 ecosystem checks) |
| Stress: 900 text-tool calls over the Link, 16 at a time | 0 failures, p50 12 ms; RSS settles at about 433 MiB |
| Stress: built-in PDF convert, searchable PDF, images to PDF, speech, 4 at a time | 32 calls, 0 failures |
| Stress: Box killed mid-burst of 1500 calls | 1 in-flight call failed cleanly; the Link relaunched Box and the rest succeeded |
| Benchmark against the 2026-10-05 baseline | startup 94.6 → 68.7 ms, warm invoke 7.5 → 7.7 ms, idle RSS 421 → 427 MiB |

Real files checked by hand: a 51-page ODT manual and a 120-row spreadsheet to
PDF, a two-page scanned PDF made searchable (text layer aligned with the
words), Zen full-page capture to PNG and paged PDF, eSpeak NG speech to MP3
and WAV.

## Limits

- Windows and macOS are compiled and tested in CI, and their installers
  build, but no window was run interactively.
- Wayland: full portal screen-selection and recording flows need a real
  desktop run; window-pin and paste-plain work on Hyprland only.
- Document to PDF keeps text, headings, lists, bold and tables, not images or
  exact layout. Old `.doc`/`.ppt` must be saved as `.docx`/`.pptx` first.
- Webpage PDF from a Firefox-family browser is the page as images (no
  selectable text); a Chromium-family browser gives a text PDF.
- Idle CPU is about one 10 ms scheduler tick every few seconds, not zero.
- On a brand-new profile the first engine check takes about 6 s; during it,
  tools that need an engine report "still checking" to other apps.
- The CLI and the desktop keep separate databases ([storage](storage.md)).
- Release gates still open: signing/notarization, signed updates and plugin
  registry, the main application license text ([release](release.md)).

## Documents

| Document | Contents |
|---|---|
| [README](../README.md) | What Box is, CLI, desktop, verification |
| [product](product.md) | Product direction |
| [architecture](architecture.md) | Boundaries, startup, performance |
| [tool API](tool-api.md), [tool catalog](tool-catalog.md), [tool matrix](tool-matrix.md), [tools/](tools) | Contract, catalog rules, per-tool status and notes |
| [providers](providers.md) | Engines and how they are found |
| [pipelines](pipeline-model.md) | Pipeline model |
| [search](search.md), [storage](storage.md) | Search and context, persistence |
| [platform](platform.md) | Platform adapters and verified scope |
| [plugin model](plugin-model.md) | WASM plugins and the worker |
| [Arcade Link](arcade-link.md) | What Box exposes and consumes, verification |
| [packaging](packaging.md), [release](release.md) | Builds, manifests, release gates |
| [security/threat-model](security/threat-model.md) | Threats and controls |
| [adr/](adr) | Architecture decisions |
| [implementation plan](implementation-plan.md), [recovery audit](recovery-audit.md), [research/](research) | Historical planning and research (dated) |
