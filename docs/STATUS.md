# Arcade Box: status

Current source version: 0.2.0, Arcade Link `v0.3.0`. The verification and
documentation-prep baseline below are historical records.

Verified 2026-10-08 on branch `arcade/link` (version 0.1.0, Arcade Link
`v0.1.0`). This page records what is implemented and how it was checked;
the other documents describe design and behavior.

The earlier Arcade Link `v0.2.0` pin added Arcade Shelf and Arcade Find to
the shared app metadata, so Connected apps lists both with a Get link. The
checks below were recorded against `v0.1.0`; the 2026-10-08 figures are not
re-measured for the new pin.

Version 0.1.2 adds **Add to Shelf** to the result action row: output files,
folders and text go to Arcade Shelf's `shelf.add` as references (see
[Arcade Link](arcade-link.md)). It is covered by core unit tests against a
mock Shelf manifest; a run against a real Shelf is not recorded here yet.

## Tools hub standard (documentation prep, 2026-10-11)

This section records the required Phase 8 target, not a claim that the runtime
work is complete. The source baseline for this documentation prep was Box
0.1.2 at `360e6e8` (Arcade Link `v0.2.0`). At that baseline, no verified
runtime-worker evidence was recorded for the newer tray-host, receipt,
self-install or settings methods.
The dated verification below remains historical evidence for the commits and
flows it names.

| Area | Required behavior and current evidence |
|---|---|
| Tray hosting | **Required; runtime verification pending.** Follow the Tools registry notifications and `tray.host` without idle polling. Keep Box's own icon when hosting is off or Link is disabled; unregister it while hosted and recreate it immediately on hand-back or connection loss. Defer registration at startup for at most 300 ms, wait up to 5 s during a Tools restart, report `status.tray`, and verify hand-back within 1 s after a crash. Do not rely on a hidden-icon constructor: prepare icon/menu data while hosted and register the OS icon only when Box owns it. |
| Linux self-install and receipts | **Required; implementation/runtime verification pending.** A real AppImage install should integrate Box under `~/Applications/Arcade`, retain its existing desktop-entry ID, create launcher/icon/CLI/autostart entries and write an atomic, mode-0600 schema-1 receipt under the Arcade data root (honoring `ARCADE_HOME`). Record the app/version/channel, method, manager, installed path, integration paths and timestamps; dev builds do not write receipts. Uninstall and repair must use receipt paths only; install/update preserve Box data and remove it only on explicit request. A dev build must not prompt or register desktop integration. |
| Standard CLI flags | **Required; not verified in this baseline.** The shared additions are `--restart`, `--install [--silent] [--no-launch] [--channel C]`, `--uninstall [--silent] [--remove-data]`, `--repair` and read-only JSON `--integration-status`. Box's tray already has a Restart action, but that does not establish the standardized CLI flag. Existing `arcade-desktop` flags and the `arcade-box` tool CLI remain documented above and in [Arcade Link](arcade-link.md). |
| Link methods and settings transfer | **Required; runtime verification pending.** Implement/advertise `app.settings`, `app.restart`, `app.menu`, `app.menu.invoke`, `app.shortcuts.set`, `app.settings.export` and `app.settings.import` against the landed Link contract; Box's app-specific menu includes **Open pipelines**. Box's portable export allowlist is pipelines, favorites and presets; history is excluded. Imports must validate and back up before applying. The current `v0.2.0` pin does not prove these newer methods. |
| Shortcut and user docs | **Required; docs/schema validation pending.** Ship task-focused `docs/user/` pages, `shortcuts.json` and generated `shortcuts.md`; validate JSON against the shared schema/vectors, check generated output and local links in CI. Keep manifest global shortcuts in canonical accelerator notation; in-app shortcuts stay in the docs snapshot. This baseline has no `docs/user/` directory. |
| Catalog and Tools e2e | **Required; onboarding verification pending.** Add Box's supported install assets and docs to the catalog through a reviewable catalog PR, and cover catalog lookup, install, update, receipt cleanup and removal in Tools/Link e2e. No such runtime/e2e verification is recorded in this status. |

The existing Linux, Windows and macOS installer builds are not evidence for the
new self-install or tray-host flows. Windows and macOS remain build-only unless
their package and interactive flows are separately run and recorded; a CI
build or installer artifact must not be described as a real desktop test.

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
