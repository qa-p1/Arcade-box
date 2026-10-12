# Arcade Box

Arcade Box brings file, media, text, web, and system tasks into one searchable desktop tool. Open it with a shortcut, find an action, and use the result in your workflow.

## Install

Install Box from **Arcade Tools → Apps**, or install it by hand using the [getting-started guide](docs/user/getting-started.md).

## Works with other Arcade apps

Box connects with Find, Lens, Look, Clipboard, Shelf, and Wheel for related tasks. See [how Box works with other Arcade apps](docs/user/with-other-apps.md).

Read the [Arcade Box user guide](docs/user/) for setup, features, shortcuts, and help.

## Development

### Run and use the CLI

The Rust CLI shares the desktop app’s tool runtime and lets you search, run implemented tools, inspect providers, and save or execute pipelines. Requirements: Rust 1.88 or newer.

    cargo run -p arcadebox -- tools
    cargo run -p arcadebox -- search hash
    cargo run -p arcadebox -- run arcade.text.case "hello arcade" --set mode=upper
    cargo run -p arcadebox -- run arcade.text.structured '{"ready":true,"items":[1,2]}' --set to=yaml
    cargo run -p arcadebox -- run arcade.developer.hash abc --set algorithm=sha256 --json
    cargo run -p arcadebox -- run arcade.convert.calculator '2 + 3 * 4'
    printf 'hello arcade' | cargo run -p arcadebox -- run arcade.text.case - --set mode=upper
    cargo run -p arcadebox -- run arcade.video.inspect --file ./clip.mp4 --json
    cargo run -p arcadebox -- providers

Many tools use optional engines: FFmpeg/ffprobe for media, libvips/ImageMagick for images, qpdf/Poppler/Ghostscript for PDFs, and yt-dlp for media acquisition. Webpage capture reuses an installed Chromium-family or Firefox-family browser. Images-to-PDF and document conversion are built in. Searchable PDF OCR uses Poppler, Tesseract, and qpdf. No Chromium, LibreOffice, img2pdf, OCRmyPDF, or Piper is bundled or silently installed by Box.

Transcription and auto-subtitles use Groq and need **GROQ_API_KEY**. Offline text-to-speech uses SAPI, say, or an installed eSpeak NG/eSpeak. OCR detects Tesseract or an available Lens provider; Engines offers an explicit Tesseract download when needed. See [Providers](docs/providers.md) and the [tool pages](docs/tools) for formats, privacy, and limits.

### Build the desktop shell

Requirements: Node.js 24 or newer, npm, and the [Tauri 2 system prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS. On Linux, install the WebKitGTK 4.1 development packages listed there.

    ./scripts/run-desktop.sh

The script builds the frontend and embedded Tauri assets, builds the desktop and plugin worker, and launches the result. On Linux it installs the local build under `${XDG_DATA_HOME:-$HOME/.local/share}/arcade-box/local-build` and registers a desktop application entry under `${XDG_DATA_HOME:-$HOME/.local/share}/applications`. It does not edit shell profiles, autostart files, or compositor configuration. Re-run the script after changing frontend assets. Desktop invocation, portal behavior, and cross-platform interaction are still being verified; see [platform](docs/platform.md) for current evidence.

The tray icon opens Settings on click. Its menu has **Open Box**, **Open Settings**, **Restart Arcade Box**, and **Quit Arcade Box**.

### Installer builds and releases

CI builds NSIS for Windows, AppImage for Linux, and a dmg for macOS, including the **arcade-box** CLI and trusted plugin worker beside the GUI. Successful CI builds on main feed the stable version and rolling nightly release workflows. Every release includes **arcade-release.json** and **SHA256SUMS.txt** for Arcade Tools and manual verification. See [packaging](docs/packaging.md).

### Verify changes

Run the CLI smoke flow and Rust suite from the repository root:

    python3 scripts/smoke-mvp.py
    cargo test --workspace --exclude arcade-desktop

The smoke script builds the CLI and exercises text conversion, JSON formatting and error reporting, SHA-256, calculator evaluation, QR generation and decoding, and a saved two-step text pipeline. It stores its database and generated files in a temporary process-scoped profile and removes them when complete.

### Repository guide

- **catalog/tools.json**: authoritative tool catalog.
- **crates/arcade-contract**: versioned requests, manifests, typed values, and results.
- **crates/arcade-core**: search, storage, grants, providers, jobs, pipelines, and first-party runtime.
- **apps/desktop**: Tauri desktop shell and Svelte Island.
- **apps/cli**: commands using the same core runtime.
- **docs/architecture.md** and **docs/implementation-plan.md**: design and execution status.
- **docs/security/threat-model.md**: trust boundaries and security cases.
- **licenses**: direct and resolved third-party license inventory.

### Arcade Link behavior

Box exposes tool and pipeline actions to connected apps. Saved pipelines can combine peer actions with Box tools, and their first stage may open a peer picker. Outbound, network, or command effects require first-run approval. Box keeps its local tools and fallbacks when used alone; Lens does not expose its recorder over Link, so Box keeps its own screen recorder. See [Arcade Link actions and pipelines](docs/arcade-link.md) for actions, cancellation, input limits, and version repair. Windows and macOS integration remains build-only until native runs verify it; see the [platform matrix](docs/platform.md).

### Contributing and license

Run `cargo fmt --all --check`, the Rust test suite, `npm run check`, and `npm run build` before proposing code. Keep manifest metadata, documentation, and tests aligned with any tool implementation. The workspace currently declares GPL-3.0-or-later as a **provisional license candidate** while the complete dependency and distribution audit is finished; see [ADR 0005](docs/adr/0005-license-policy.md). No release should rely on that declaration without the completed audit and a checked-in license text.
