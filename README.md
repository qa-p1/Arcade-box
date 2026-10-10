# Arcade Box

Arcade Box is an open-source desktop action surface for everyday file, media, text, and system tasks. The flow is **shortcut → type → run → use the result → dismiss**. Its Rust core is shared by the Tauri desktop app and CLI; the tool catalog is the source for search, typed input/output, privacy labels, and implementation status.

The current catalog contains 109 implemented tools. Runtime availability still depends on the input, operating system, permissions and optional engines. See the [tool readiness matrix](docs/tool-matrix.md) and [current status and documentation index](docs/STATUS.md) for verified behavior and remaining limits.

## Run and use the CLI

The Rust CLI shares the desktop app's tool runtime and lets you search, run implemented tools, inspect providers, and save or execute pipelines. Requirements: Rust 1.95 or newer.

```sh
# Show available tools and their status, then find a tool.
cargo run -p arcadebox -- tools
cargo run -p arcadebox -- search hash

# Run text and data tools.
cargo run -p arcadebox -- run arcade.text.case "hello arcade" --set mode=upper
cargo run -p arcadebox -- run arcade.text.structured '{"ready":true,"items":[1,2]}' --set to=yaml
cargo run -p arcadebox -- run arcade.developer.hash abc --set algorithm=sha256 --json
cargo run -p arcadebox -- run arcade.convert.calculator '2 + 3 * 4'

# Read text from stdin, or pass selected files to file tools.
printf 'hello arcade' | cargo run -p arcadebox -- run arcade.text.case - --set mode=upper
cargo run -p arcadebox -- run arcade.video.inspect --file ./clip.mp4 --json

# See detected optional system providers.
cargo run -p arcadebox -- providers
```

Many tools use optional engines: FFmpeg/ffprobe for media, libvips/ImageMagick for images, qpdf/Poppler/Ghostscript for PDFs, and yt-dlp for media acquisition. Webpage capture reuses an installed Chromium-family or Firefox-family browser. Images-to-PDF and document conversion are built in; searchable PDF OCR uses Poppler, Tesseract and qpdf. No Chromium, LibreOffice, img2pdf, OCRmyPDF or Piper is bundled or installed by Box.

Transcription and auto-subtitles use Groq and need `GROQ_API_KEY`; offline text-to-speech uses SAPI, `say`, or an installed eSpeak NG/eSpeak. OCR detects Tesseract or an available Lens provider; Engines offers an explicit Tesseract download when needed. [Providers](docs/providers.md) and the [tool pages](docs/tools) describe formats, privacy and limits.

## Desktop shell

Requirements: Node.js 24 or newer, npm, and the [Tauri 2 system prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS. On Linux, install the WebKitGTK 4.1 development packages listed there. Build and launch the desktop shell with:

```sh
./scripts/run-desktop.sh
```

The script builds the frontend and embedded Tauri assets, builds the desktop and plugin worker, and launches the result. On Linux it installs the local build under `${XDG_DATA_HOME:-$HOME/.local/share}/arcade-box/local-build` and registers a desktop application entry under `${XDG_DATA_HOME:-$HOME/.local/share}/applications`; it does not edit shell profiles, autostart files, or compositor configuration. Re-run the script after changing frontend assets. Desktop invocation, portal behavior, and cross-platform interaction are still being verified; see the [platform matrix](docs/platform.md) for current evidence.

The tray icon opens Settings on click. Its menu is the one every Arcade app has: **Open Box**, **Open Settings**, **Restart Arcade Box** and, below a separator, **Quit Arcade Box**.

## Works with other Arcade apps

With installed, enabled peers, Box can use Arcade Lens for screen selection and
OCR, preview results in Arcade Look, send them to your devices through Arcade
Clipboard, keep result files and text on Arcade Shelf, and add tools, presets
or pipelines to Arcade Wheel. Saved pipelines
can combine peer actions with Box tools; the first stage may open a picker.
Outbound/network/command effects need first-run approval. Connected apps settings
control each connection. Box works on its own with its existing local tools and
fallbacks. Lens exposes no recorder action over Link, so Box keeps its own screen recorder.

[Arcade Link actions and pipelines](docs/arcade-link.md) describes the exposed and
consumed actions, cancellation, input limits and version repair. Windows/macOS
integration remains build only until native runs verify it; see the
[platform evidence](docs/platform.md).

## Installer builds

CI builds NSIS (Windows), AppImage (Linux), and dmg (macOS), including the
`arcade-box` CLI and trusted plugin worker beside the GUI. Successful CI builds
on `main` feed the stable `v<version>` and rolling `nightly` release workflow.
Every release includes `arcade-release.json` and `SHA256SUMS.txt` for Arcade Tools
and manual verification. See [packaging](docs/packaging.md) for local commands
and current validation limits.

## Verification

Run the real CLI smoke flow and Rust suite from the repository root:

```sh
python3 scripts/smoke-mvp.py
cargo test --workspace --exclude arcade-desktop
```

The smoke script builds the CLI and exercises text conversion, JSON formatting and error reporting, SHA-256, calculator evaluation, QR generation and decoding, and a saved two-step text pipeline. It stores its database and generated files in a temporary process-scoped profile and removes them when complete.

## Repository guide

- [`catalog/tools.json`](catalog/tools.json): authoritative tool catalog.
- [`crates/arcade-contract`](crates/arcade-contract): versioned requests, manifests, typed values, and results.
- [`crates/arcade-core`](crates/arcade-core): search, storage, grants, providers, jobs, pipelines, and first-party runtime.
- [`apps/desktop`](apps/desktop): Tauri desktop shell and Svelte Island.
- [`apps/cli`](apps/cli): commands using the same core runtime.
- [`docs/architecture.md`](docs/architecture.md), [`docs/implementation-plan.md`](docs/implementation-plan.md): design and execution status.
- [`docs/security/threat-model.md`](docs/security/threat-model.md): trust boundaries and security cases.
- [`licenses`](licenses): direct and resolved third-party license inventory.

## Contributing and license

Run `cargo fmt --all --check`, the Rust test suite, `npm run check`, and `npm run build` before proposing code. Keep manifest metadata, documentation, and tests aligned with any tool implementation. The workspace currently declares GPL-3.0-or-later as a **provisional license candidate** while the complete dependency and distribution audit is finished; see [ADR 0005](docs/adr/0005-license-policy.md). No release should rely on that declaration without the completed audit and a checked-in license text.
