# Tool catalog

[`catalog/tools.json`](../catalog/tools.json) is the source of truth for tool discovery metadata. Search, categories, documentation, plugin details, and readiness views should consume this registry rather than maintain a second frontend list.

The document has `schemaVersion` for catalog serialization and `apiVersion` for tool compatibility. A tool's semantic `version` changes when its behavior or public options change; API compatibility is versioned separately. Stable IDs use the `arcade.<family>.<tool>` form and are referenced by saved pipelines, aliases, history, and deep links. Once released, an ID is never reused for a different action. The catalog may grow beyond the initial product list; its current entries, rather than a hardcoded count here, define what ships.

Each record contains names and aliases for search, MIME-like input and output types, provider capability IDs, the privacy class, a permission declaration, execution runtime, implementation status, and platform status. Array types use `[]`; a pipeline transports file/artifact references rather than serializing large contents. Provider IDs describe capabilities, not executable paths.

Read each tool's current `status` and platform values from the catalog. The generated [`tool matrix`](tool-matrix.md) summarizes those records and their verification metadata. `planned` describes intended work only. `partial` records a real implementation with documented capability gaps. `conditional` means a platform capability depends on a desktop portal, compositor, provider, or OS permission. `unsupported` records a known boundary. `implemented` requires a real backend, result handling, documented behavior, and suitable verification. A card in search does not imply an implementation.

`LOCAL` means the operation needs no outbound network; `NETWORK` means network access is inherent; `CLOUD` means user-provided content may be sent to a remote processor. A specific provider selection may make an optional operation more private, but the user interface must derive the effective disclosure from the provider actually selected and show it before content leaves the device.

The catalog schema is [`catalog/tools.schema.json`](../catalog/tools.schema.json). Rust consumers also validate critical invariants, including supported API version, stable ID shape, nonempty user-facing text, at least one output type, and unique IDs. Unknown fields remain forward-compatible. Catalog changes should be reviewed for IDs, aliases, types, privacy, permissions, provider capabilities, and platform claims together.

Per-tool notes live under [`docs/tools`](tools). They cover [batch rename](tools/batch-rename.md) (which also sanitizes file names), [duplicate finding](tools/duplicate-finder.md), [file and folder comparison](tools/file-folder-compare.md), [folder sizes](tools/folder-size-analyzer.md), [password generation](tools/password-generator.md), [encryption](tools/file-text-encryption.md), the PDF, image, video, and audio families, and the web and network tools. Codec and container choices declare their FFmpeg capabilities with `requires`, so the form only offers what the installed FFmpeg can produce.

Tools added in the regrouped catalog keep their documentation in the form help text and in [providers](providers.md):

- **Speech:** transcribe and translate (`arcade.audio.transcribe`), auto-subtitles (`arcade.video.auto-subtitles`), and the transcript fallback use Groq Whisper; text to speech uses the OS voice (SAPI / `say` / installed eSpeak NG or eSpeak); noise removal uses RNNoise or FFmpeg's FFT denoiser; vocal separation uses Demucs when installed.
- **PDF:** sign (typed or image signature, visible only), fill AcroForm fields (list, then fill), and compress to a target size with Ghostscript.
- **Images:** HEIC/AVIF input, compress to a target size, passport photos and print sheets, combine, watermark, and favicon sets use libvips and ImageMagick.
- **Web:** video transcript and thumbnail (yt-dlp), webpage to PDF or full-page screenshot (an installed Chromium-family or Firefox-family browser; the latter produces image-only PDFs), and download all page images (static HTML only). The website checker answers "is this site down?" with curl and the system `ping`.
- **Everyday utilities:** emoji picker, colour converter with WCAG contrast, percentage/discount/tax/tip, loan EMI, timer and stopwatch (in the window), random picker, lorem ipsum, and Excel ↔ CSV.
