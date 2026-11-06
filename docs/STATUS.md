# Arcade Box: status

## 0.4.3 dashboard usability follow-up (2026-11-06)

The dashboard clips its outer frame, but the main content pane had no scroll
container. Long catalogs and Settings were cut off. The pane now scrolls with
the mouse wheel and keyboard, independently of the category sidebar. Changing
pages/categories returns it to the heading. The search shortcut also opens the
catalog from pages where no search field is mounted.

The previous layout gate only checked horizontal overflow. Its expanded checks
exercise wheel/PageDown/End, reaching the bottom, page changes, sidebar focus
and search from Settings in both themes and three dashboard sizes, with larger
text at minimum size. The actual WebView package gate also requires a bounded,
scrollable native catalog and Settings pane before running Calculator.

## 0.4.1 candidate (2026-11-02)

- Follow-up patch: pins Link and the design kit to `v0.4.15`; secondary text
  uses `muted`, status/accent foregrounds use the kit's readable `*-ink`
  tokens, and `.editorconfig` records the existing formatting conventions.
- Local: Svelte/TypeScript reports 0 errors and 0 warnings; all 12 frontend
  tests and the production build pass. Version, 53 vendored hashes, CSS
  variables, docs/shortcuts and Rust formatting checks pass. Cargo.lock changes
  only Box's version and the Link dependency.
- CI run 41491527927 passed Rust tests/builds on all three operating systems
  and Linux's layout check, then correctly failed the stale license inventory.
  The inventory now records the locked Link 0.4.15 source. The design sync
  also completed locally, including rendering both tray icons (unchanged bytes).
- Remaining release gates: full Rust and layout checks, three-OS CI and
  installers, then `tag.yml` and stable `release.yml`, checksums, manifest and
  Tools update verification. Native desktop checks listed below remain separate.
- Published stable version is still
  [v0.4.0](https://github.com/qa-p1/Arcade-box/releases/tag/v0.4.0), from successful
  [Release run 40789177850](https://github.com/qa-p1/Arcade-box/actions/runs/40789177850).

## 0.4.1 runtime follow-up (2026-11-03)

The released 0.4.0 sent an invalid Lua string to Hyprland (reproduced on
0.56.2), disabling the surface rules and shortcut. Its dashboard also lacked
Tauri event permissions, so page and tool handoffs could not initialize.
The floating Island, separate dashboard, toolkit and existing tools are retained.

The rule now uses a Lua literal regex; Linux CI parses that exact source.
The AppImage gate now requires actual native catalog, Settings handoff and a
Calculator result of 42 through the dashboard/Island UI, rather than process
survival alone. Local and CI results must pass before release.

## 0.4.0 (2026-10-28)

- Source version 0.4.0 on Arcade Link `v0.4.0`. The overhaul (BOX.md) adopts
  the design kit, splits the dashboard into its own window, moves to
  `dev.arcade.Box` and `arcade/box`, fixes B1–B9 and adds `scripts/check`,
  the one-version check and the `tag.yml` → `release.yml` flow.
- Checked on Linux in this container: `scripts/check` (including the layout
  check at every Island and dashboard size, light and dark), the B1–B9
  regression tests, the data migration from a real 0.3.5 profile (isolated
  and XDG), the window class `dev.arcade.Box` and the webview folder under
  Xvfb. Windows and macOS are checked by CI only.
- Still needs a real desktop: the global shortcut opening the Island on
  Hyprland, GNOME, KDE and the other platforms; tray behaviour; overlay
  timing; start at login on macOS and Windows; and Tools updating Box from
  0.3.5 on a real profile.

## Historical release status (2026-10-18)

- Latest release: [v0.3.5](https://github.com/qa-p1/Arcade-box/releases/tag/v0.3.5), published 2026-10-19 (the Island takes keyboard focus on X11; see CHANGELOG.md). The CI and e2e notes below were recorded for the earlier v0.2.0.
- Previously released: [v0.3.2](https://github.com/qa-p1/Arcade-box/releases/tag/v0.3.2), published 2026-10-19 (released every inherited descriptor at start, which also cut off helpers from the app's own image).
- Previously released: [v0.3.1](https://github.com/qa-p1/Arcade-box/releases/tag/v0.3.1), published 2026-10-18 (fixes for what verifying the release before it found).
- Previously released: [v0.3.0](https://github.com/qa-p1/Arcade-box/releases/tag/v0.3.0), published 2026-10-18 (fixes from the October audit and stress test).
- Previously released: [v0.2.1](https://github.com/qa-p1/Arcade-box/releases/tag/v0.2.1), published 2026-10-15.
- Previously released: [v0.2.0](https://github.com/qa-p1/Arcade-box/releases/tag/v0.2.0) with a Linux self-installing AppImage, a per-user Windows installer, a macOS dmg, `arcade-release.json` and `SHA256SUMS.txt`.
- Merged to `main` from PR #3 (a9e806f) after CI passed on Linux, Windows and macOS: [run 38600230680](https://github.com/qa-p1/Arcade-box/actions/runs/38600230680). That covers the build, tests, clippy or analyzers, installer smoke tests on each OS, and the Linux AppImage install, repair and uninstall check in a temporary profile.
- Ecosystem e2e (Arcade Link `tools/e2e.py`, real CI builds of all eight apps): install, tray hosting (hosted by Tools, own icon back within 1 s after Tools is killed, no flash on restart) and shortcuts pass. Tools 0.2.0 adopts, updates, rolls back, uninstalls and backs up Box.
- Not yet verified: a real Windows or macOS desktop (CI runners only), and the owner's real-desktop checks. Builds are unsigned, so SmartScreen and Gatekeeper ask for approval.

Current source version: 0.3.5, Arcade Link commit `3bbbc5d`. The verification and
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
[Arcade Link](dev/link.md)). It is covered by core unit tests against a
mock Shelf manifest; a run against a real Shelf is not recorded here yet.

## Tools hub verification (2026-10-14)

The current source implements the Tools hub contract using Arcade Link `v0.3.0`.
[CI at `02ff795`](https://github.com/qa-p1/Arcade-box/actions/runs/38524774283)
passed on Linux, Windows and macOS, including the packaged installer lifecycle.
The dated measurements below remain historical performance evidence.

| Area | Implementation and verification |
|---|---|
| Tray hosting | Linux unregisters its StatusNotifierItem while hosted and recreates it on hand-back. `tests/tray-registration.py` verifies registration, host crash, restart, opt-out, menu actions and repeated transitions against a private watcher. Windows/macOS tray behavior is checked in code and CI, without a recorded interactive desktop run. |
| Install lifecycle | The real AppImage, per-user NSIS setup and universal macOS DMG run isolated install/uninstall checks in CI. Receipts, launcher, icons, CLI, repair, data retention and CLI symlink cleanup are covered by the installer tests and smoke scripts. macOS updates retain the adopted Applications folder and restore the previous bundle if replacement fails. |
| Link and settings | Box advertises the standard settings, restart, menu, shortcut and settings-transfer methods. Core mock-peer tests cover peer actions and the settings export allowlist. Exported pipelines may contain user-written commands; review them before sharing a backup. |
| User documentation | `docs/user/`, generated shortcuts and pinned shared assets are validated by `scripts/check-docs.py`. |
| Tools lifecycle | Tools' isolated hub tests exercise standalone adoption, update, rollback, failure restoration and uninstall. An interactive Windows/macOS desktop run is still outstanding. |

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
  Clipboard, Wheel, Shelf and Tools integrations listed in [Arcade Link](dev/link.md).
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
- Release gates still open: signing/notarization, signed updates and plugin
  registry, the main application license text ([release](dev/release.md)).

## Documents

| Document | Contents |
|---|---|
| [README](../README.md) | What Box is, CLI, desktop, verification |
| [product](dev/history/product.md) | Product direction |
| [architecture](dev/architecture.md) | Boundaries, startup, performance |
| [tool API](dev/tool-api.md), [tool catalog](tool-catalog.md), [tool matrix](tool-matrix.md), [tools/](dev/history/tools) | Contract, catalog rules, per-tool status and notes |
| [providers](dev/providers.md) | Engines and how they are found |
| [pipelines](dev/pipeline-model.md) | Pipeline model |
| [search](dev/search.md), [storage](dev/storage.md) | Search and context, persistence |
| [platform](dev/platform.md) | Platform adapters and verified scope |
| [plugin model](dev/plugin-model.md) | WASM plugins and the worker |
| [Arcade Link](dev/link.md) | What Box exposes and consumes, verification |
| [packaging](dev/packaging.md), [release](dev/release.md) | Builds, manifests, release gates |
| [security/threat-model](dev/threat-model.md) | Threats and controls |
| [adr/](dev/adr) | Architecture decisions |
| [implementation plan](dev/history/implementation-plan.md), [recovery audit](dev/history/recovery-audit.md), [research/](dev/history/research) | Historical planning and research (dated) |
