<p align="center">
  <img src="apps/desktop/src-tauri/icons/icon.svg" width="112" alt="Arcade Box icon">
</p>

<h1 align="center">Arcade Box</h1>

<p align="center">File, media, text, web and system tasks in one searchable desktop tool.</p>

<p align="center">
  <a href="https://github.com/qa-p1/Arcade-box/releases/latest"><img src="https://img.shields.io/github/v/release/qa-p1/Arcade-box" alt="Latest release"></a>
  <a href="https://github.com/qa-p1/Arcade-box/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/qa-p1/Arcade-box/ci.yml?branch=main&label=CI" alt="CI status"></a>
</p>

---

Arcade Box brings file, media, text, web, and system tasks into one searchable desktop tool. Open it with a shortcut, find an action, and use the result in your workflow. It is for people who would otherwise reach for a dozen single-purpose converters and utilities.

It runs on **Linux** (x86_64; X11 and Wayland), **Windows** (x64) and **macOS** (universal: Apple Silicon and Intel). Linux flows run under isolated Xvfb and selected Hyprland flows are verified; Windows and macOS are built and tested in CI, but interactive desktop behavior is unverified.

<p align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/user/images/island-results-dark.png"><img src="docs/user/images/island-results-light.png" width="560" alt="The Island: searching “compress” lists matching tools grouped by category"></picture></p>

<p align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/user/images/tool-run-dark.png"><img src="docs/user/images/tool-run-light.png" width="560" alt="A tool run in the Island: Change Case with its input, options and result"></picture></p>

<p align="center"><picture><source media="(prefers-color-scheme: dark)" srcset="docs/user/images/dashboard-tools-dark.png"><img src="docs/user/images/dashboard-tools-light.png" width="760" alt="The dashboard window on its Tools page, with the sidebar of pages and categories"></picture></p>

## Download and install

Download the file for your system from [Releases](https://github.com/qa-p1/Arcade-box/releases). Or install it from **[Arcade Tools](https://github.com/qa-p1/Arcade-tools) → Apps**, which can also update and remove it.

| Platform | File | How to install |
|---|---|---|
| Linux x86_64 | `Arcade-Box_<version>_amd64.AppImage` | Run it. The first run asks **Install Arcade Box?** and installs to `~/Applications/Arcade/`, adds a menu entry, the `arcade-desktop` and `arcade-box` (terminal tools) commands in `~/.local/bin` and an optional start at login. **Just run it once** skips installing. |
| Windows x64 | `Arcade-Box_<version>_x64-setup.exe` | Run Setup (NSIS, per-user, no admin). Adds a Start menu entry, an optional desktop shortcut and start at login, and an `arcade-desktop` command on the user PATH. |
| macOS (universal) | `Arcade-Box_<version>_universal.dmg` | Open the dmg and drag Arcade Box to Applications. Run from the dmg or Downloads, it offers to move itself to Applications. |

- Scripted install: `arcade-desktop --install --silent` (add `--no-launch`, or `--channel C`). `--repair` rewrites the launcher, command and login entries; `--integration-status` prints the integration as JSON. A silent install removes the AppImage it was started from only when that file is in your Downloads folder; add `--keep-download` to keep it there too. Files elsewhere are never removed.
- Every installer also contains the `arcade-box` terminal tool runtime (alias `arcadebox`) and the trusted plugin worker beside the desktop app. On Linux the install links `arcade-box` in `~/.local/bin` (unless another program already uses that name) and removes it on uninstall; on Windows and macOS run it from the installation folder (`Arcade Box.app/Contents/MacOS` on macOS).
- If `~/.local/bin` is not on PATH, Box shows the exact line to add for your shell.
- Builds are unsigned. On Windows, SmartScreen may warn: choose **More info → Run anyway**. On macOS the app is not notarized: right-click it and choose **Open** the first time.
- If FUSE is missing on Linux, run the AppImage with `--appimage-extract-and-run`.

Releases are made by hand from a tag on `main` (`tag.yml`, then `release.yml`); a rolling nightly pre-release can be refreshed from `main`. Every release includes `arcade-release.json` and `SHA256SUMS.txt` for Arcade Tools and manual verification. See [Getting started](docs/user/getting-started.md) and [packaging](docs/dev/packaging.md).

## Features

Box has 109 catalog tools. Availability depends on your input, platform, permissions and optional programs.

- **PDFs:** merge, split, reorder, rotate, compress, sign, fill forms, watermark, protect or unlock; images to PDF and back; searchable PDF OCR.
- **Images:** convert, resize, crop, compress, combine, remove backgrounds, passport photos, favicons, upscale, redact, strip metadata, OCR, palettes, compare.
- **Video and audio:** convert, compress to a target size, trim, crop, join, extract audio or frames, GIF/WebP, subtitles, normalize loudness, reduce noise, speed and pitch, metadata, transcription and text-to-speech.
- **Web:** download media, transcripts and thumbnails, save a page as PDF or screenshot, extract page text, WHOIS and public IP.
- **Text and data:** diff, case, clean, JSON/YAML/TOML/XML conversion, CSV/TSV, Markdown/HTML, calculators and unit, currency, time-zone and color conversion.
- **Developer utilities:** regex, number bases, encodings, hashes, UUIDs/ULIDs, JWTs, cron expressions, QR codes and barcodes.
- **Files and desktop:** batch rename, duplicates, folder sizes, encryption, screenshots, screen recording, color picker, ruler, opt-in clipboard history, plain-text paste, window pinning and processes.
- **Pipelines:** save compatible actions as a pipeline and run it again from search, with progress and jobs that keep going while the Island is closed.
- **Plugins:** community plugins run in a restricted WebAssembly worker. Built-in tools stay available without them.

Many tools use optional engines: FFmpeg/ffprobe for media, libvips/ImageMagick for images, qpdf/Poppler/Ghostscript for PDFs, and yt-dlp for media acquisition. Webpage capture reuses an installed Chromium-family or Firefox-family browser. Images-to-PDF and document conversion are built in. Searchable PDF OCR uses Poppler, Tesseract, and qpdf. No Chromium, LibreOffice, img2pdf, OCRmyPDF, or Piper is bundled or silently installed by Box.

Transcription and auto-subtitles use Groq and need `GROQ_API_KEY`. Offline text-to-speech uses SAPI, say, or an installed eSpeak NG/eSpeak. OCR detects Tesseract or an available Lens provider; Engines offers an explicit Tesseract download when needed. See [Features](docs/user/features.md), [Providers](docs/dev/providers.md) and the [tool pages](docs/dev/history/tools) for formats, privacy, and limits.

## Works with other Arcade apps

Box works on its own and keeps its local tools and fallbacks when used alone. It connects to other installed Arcade apps automatically over Arcade Link, **without needing Arcade Tools**. Turn this on or off with **Connect with other Arcade apps** in Box's Connected apps settings, with a switch per app.

| App | What Box can do with it |
|---|---|
| Arcade Lens | Ask Lens to select part of the screen, recognize it, or inspect a captured image. Box keeps its own screen recorder. |
| Arcade Look | Preview supported files and results in Look. |
| Arcade Clipboard | Send supported text and files to your paired devices. |
| Arcade Shelf | Add result files, folders or text to Shelf as references. |
| Arcade Wheel | Add Box tools, presets or saved pipelines to Wheel. |
| Arcade Find | Find can call Box tools and saved pipelines from its action list. |
| Arcade Tools | Install, update or remove Box from its **Apps** tab. |

Box also provides tool and pipeline actions to other apps. Saved pipelines can combine peer actions with Box tools, and their first stage may open a peer picker. Outbound, network or command effects require first-run approval.

When Arcade Tools is running, its tray icon hosts Box's menu, including **Open pipelines**. When Tools quits, crashes or turns hosting off, Box's own icon returns at once. With **Connect with other Arcade apps** off, Box always shows its own icon.

See [how Box works with other Arcade apps](docs/user/with-other-apps.md) and [Arcade Link actions and pipelines](docs/dev/link.md).

## Usage

Press **Ctrl+Alt+Space** (**⌃⌥Space** on macOS) to show or hide the Island. Type a task such as **compress PDF**, **resize image** or **JSON**, choose the action and give it an input. The first time you run a tool, Box explains any permission, external program or network access it needs.

| Key | Action |
|---|---|
| Ctrl+K or / | Focus search |
| ↑ / ↓, Enter | Choose and run a result |
| Ctrl+Enter | Run the selected tool |
| Ctrl+O | Choose an input file |
| Ctrl+Shift+C | Copy the current result |
| Alt+← | Back from a tool |
| Escape | Clear the search or close the Island |

On macOS, Cmd replaces Ctrl. See the [shortcut reference](docs/user/shortcuts.md).

Clicking the tray icon opens the dashboard (on macOS a click shows the menu). The menu has **Show or hide Arcade Box**, **Open dashboard**, **Open pipelines**, **Settings**, **Restart Arcade Box** and **Quit Arcade Box**. Closing the dashboard leaves Box running in the tray.

### Desktop app: `arcade-desktop`

```
arcade-desktop                   Start Arcade Box in the tray (or show the Island if it is running)
arcade-desktop --background      Start in the tray without showing anything
arcade-desktop --show            Show the Island
arcade-desktop --hide            Hide the Island
arcade-desktop --toggle          Show or hide the Island
arcade-desktop --dashboard       Open the dashboard
arcade-desktop --settings        Open Settings
arcade-desktop --pipelines       Open Pipelines
arcade-desktop --restart         Restart the running instance
arcade-desktop --quit            Quit the running instance
arcade-desktop --install [--silent] [--no-launch] [--keep-download] [--channel C]
                                 Install this AppImage / macOS app bundle
arcade-desktop --uninstall [--silent] [--remove-data]
                                 Remove this installation via its receipt
arcade-desktop --repair          Rewrite the launcher, command and login entries
arcade-desktop --integration-status
                                 Print the integration as JSON (no side effects)
arcade-desktop --status          Print whether Box is running, its version and Link state (JSON)
arcade-desktop --version         Print the version
arcade-desktop --arcade-manifest Print the Arcade Link manifest (no side effects)
arcade-desktop --arcade-invoke   Run one Arcade Link request from stdin, without any UI
```

### Terminal tools: `arcadebox`

The Rust CLI `arcadebox` (canonical alias `arcade-box`) shares the desktop app's tool runtime and lets you search, run implemented tools, inspect providers, and save or execute pipelines. From a source checkout:

```sh
cargo run -p arcadebox -- tools
cargo run -p arcadebox -- search hash
cargo run -p arcadebox -- run arcade.text.case "hello arcade" --set mode=upper
cargo run -p arcadebox -- run arcade.text.structured '{"ready":true,"items":[1,2]}' --set to=yaml
cargo run -p arcadebox -- run arcade.developer.hash abc --set algorithm=sha256 --json
cargo run -p arcadebox -- run arcade.convert.calculator '2 + 3 * 4'
printf 'hello arcade' | cargo run -p arcadebox -- run arcade.text.case - --set mode=upper
cargo run -p arcadebox -- run arcade.video.inspect --file ./clip.mp4 --json
cargo run -p arcadebox -- providers
```

The CLI and the desktop app use the same database, so saved pipelines, favorites and history are shared.

## Settings and data

Box stores application metadata (settings, favorites, usage, history, aliases, pipelines, jobs and provider choices) in `arcade.sqlite3`. From 0.4.0 the desktop app and the CLI share one folder: `~/.local/share/arcade/box` on Linux, `%APPDATA%\Arcade\Box` on Windows and `~/Library/Application Support/Arcade/Box` on macOS (`$ARCADE_HOME/.local/share/arcade/box` when `ARCADE_HOME` is set). The first 0.4.0 start moves the older `dev.arcadebox.app` and CLI folders there; if both held a different `arcade.sqlite3`, the desktop's copy is used and the CLI's is left where it was, as the log says. Settings › About shows the active folder. History and job records omit input content, source paths, output values and error text.

Opt-in clipboard history is stored separately in `clipboard-history.json` beside the desktop database (retention 1, 7, 30 or 90 days; default 7). Groq credentials come from `GROQ_API_KEY` or the application data directory's `.env`, which is plaintext; keep it private. See [storage and retention](docs/dev/storage.md).

Arcade Tools can export and import Box's favorites, saved pipelines and preferences (theme, onboarding state, global shortcut). Imports are validated, the current settings are backed up, and everything rolls back if a step fails. **Never exported:** history, job outputs or device state.

## Uninstall

- **Linux:** run `arcade-desktop --uninstall`, choose **Uninstall** from its launcher entry, or remove Box in Arcade Tools.
- **Windows:** **Settings → Apps** (Apps & features), or Arcade Tools.
- **macOS:** `arcade-desktop --uninstall` removes the installation recorded in its receipt, or remove Box in Arcade Tools.

Your settings stay unless you choose to remove them (`--remove-data`).

## Platform notes

- On Linux, Box prefers XDG Desktop Portals for global shortcuts, screenshots, screencasting and file selection, and PipeWire for capture. X11 remains an explicit target.
- Box's window-pin and paste-plain actions have been exercised on Hyprland. Full Wayland screen selection and recording still need a desktop run.
- Plain-text paste on X11 requires xdotool; pinning another app's window on X11 needs wmctrl/EWMH. Both are unavailable on Wayland compositors other than Hyprland.
- macOS window pinning is hidden because no implementation exists. Screen selection needs macOS 14+, screen recording macOS 15+.
- Windows and macOS integration remains build-only until native runs verify it. See the [platform matrix](docs/dev/platform.md).

## Development

The desktop app is Tauri 2 on the system webview with a Svelte 5 interface on the Arcade design kit, vendored from Arcade Link v0.4.0 into `apps/desktop/frontend/src/design/`. Toolchains are pinned: Rust 1.88 (`rust-toolchain.toml`) and Node 24. Install the [Tauri 2 system prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS; on Linux that means the WebKitGTK 4.1 packages listed in [`.github/workflows/ci.yml`](.github/workflows/ci.yml).

```sh
npm ci --prefix apps/desktop/frontend
scripts/dev                 # run from source (scripts/dev --ui: the UI alone on the mock backend)
scripts/check               # exactly what CI runs on this OS
scripts/package             # this OS's installer, then install, repair and uninstall it in a disposable profile
python3 scripts/sync-design.py --link ../Arcade-Link    # refresh the design kit from the pinned tag
python3 scripts/check-version.py --set 0.4.1            # bump the one version
```

`scripts/check` runs the frontend type and accessibility checks, unit tests, the layout check (no element wider than its box, at every Island and dashboard size, light and dark; Linux, needs `npx playwright install chromium`), the undefined-CSS-variable, version, docs and vendored-hash checks, `cargo fmt --check`, `clippy -D warnings`, the Rust tests in a throwaway profile, the tool matrix and the licence inventory. CI runs it on Linux, Windows and macOS, then builds and verifies the AppImage, NSIS and universal dmg installers.

`./scripts/run-desktop.sh` builds Box and launches it in the tray; on Linux it installs that build under `${XDG_DATA_HOME:-$HOME/.local/share}/arcade-box/local-build` with a desktop entry, and changes no shell profile, autostart file or compositor configuration. `python3 scripts/smoke-mvp.py` builds the CLI and runs a short smoke flow (text, JSON, SHA-256, calculator, QR and a saved two-step pipeline) in a temporary profile.

Developer notes are in [docs/dev](docs/dev/): [architecture](docs/dev/architecture.md), [Link](docs/dev/link.md), [packaging and releases](docs/dev/packaging.md), [testing](docs/dev/testing.md) and [decisions](docs/dev/decisions.md).

### Repository guide

- [catalog/tools.json](catalog/tools.json): authoritative tool catalog.
- [crates/arcade-contract](crates/arcade-contract): versioned requests, manifests, typed values, and results.
- [crates/arcade-core](crates/arcade-core): search, storage, grants, providers, jobs, pipelines, and first-party runtime.
- [apps/desktop](apps/desktop): Tauri desktop shell and Svelte Island.
- [apps/cli](apps/cli): commands using the same core runtime.
- [docs/dev/architecture.md](docs/dev/architecture.md): design; [docs/dev/history](docs/dev/history/): the original spec, plans and audits.
- [docs/dev/threat-model.md](docs/dev/threat-model.md): trust boundaries and security cases.
- [licenses](licenses): direct and resolved third-party license inventory.

## Documentation

- [User guide](docs/user): [getting started](docs/user/getting-started.md), [features](docs/user/features.md), [shortcuts](docs/user/shortcuts.md), [FAQ](docs/user/faq.md), [troubleshooting](docs/user/troubleshooting.md), [with other apps](docs/user/with-other-apps.md)
- [Arcade Link actions and pipelines](docs/dev/link.md)
- [Tool catalog](docs/tool-catalog.md), [tool matrix](docs/tool-matrix.md), [providers](docs/dev/providers.md)
- [Platform matrix](docs/dev/platform.md), [storage](docs/dev/storage.md), [packaging](docs/dev/packaging.md)
- [Status](docs/STATUS.md)
- [Changelog](CHANGELOG.md)

## Contributing

Run `scripts/check` before proposing code. Keep manifest metadata, documentation, and tests aligned with any tool implementation.

## License

Arcade Box is licensed under the GNU General Public License v3.0 or later; see [LICENSE](LICENSE). Third-party licences are listed in [licenses](licenses/), and the policy is [ADR 0005](docs/dev/adr/0005-license-policy.md).

---

<p align="center">
  Part of the <b>Arcade</b> suite:
  <a href="https://github.com/qa-p1/Arcade-Find">Find</a> ·
  <a href="https://github.com/qa-p1/Arcade-lens">Lens</a> ·
  <a href="https://github.com/qa-p1/Arcade-look">Look</a> ·
  <a href="https://github.com/qa-p1/Arcade-box">Box</a> ·
  <a href="https://github.com/qa-p1/Arcade-clipboard">Clipboard</a> ·
  <a href="https://github.com/qa-p1/Arcade-Shelf">Shelf</a> ·
  <a href="https://github.com/qa-p1/Arcade-wheel">Wheel</a> ·
  <a href="https://github.com/qa-p1/Arcade-tools">Tools</a> ·
  <a href="https://github.com/qa-p1/Arcade-catalog">Docs</a>
</p>
