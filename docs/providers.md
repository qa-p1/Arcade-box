# Providers and dependencies

Tools request named capabilities, never an executable path. The provider broker probes candidate programs and libraries, records the chosen provenance, runs a bounded safe health/version/capability check, and offers compatible choices to the user. Prefer a compatible system provider; managed providers live under Arcade Box application data, are independently verified, removable, and never replace global installations.

Discovery checks normal `PATH`, known platform application/package locations, and user-configured locations. It must resolve the absolute path, inspect identity and version, probe required behavior without shell invocation, record provenance, and warn on suspicious resolution. Calls use structured argument arrays, a sanitized environment, controlled working directory, restricted private temporary files, cancellation, output limits, and timeouts.

| Capability family | Engine | Reuse and fallback policy | Status |
|---|---|---|---|
| Audio/video and probing | FFmpeg / ffprobe | Reuse a compatible system installation. Probe identity and version, and record the encoders (`encoder:libx264`, `libx265`, `libsvtav1`/`libaom-av1`, `libvpx-vp9`, `prores_ks`, `mjpeg`, `libwebp_anim`, …), muxers (`mux:mp4`, `matroska`, `webm`, `mov`, …), and filters (`filter:subtitles`, `palettegen`, `loudnorm`, `silencedetect`, `rubberband`, `arnndn`, `afftdn`, `stereotools`, `drawtext`, …) that the tools and form choices require. | Implemented on Linux; Windows/macOS pending |
| Public media acquisition | yt-dlp | Reuse a compatible system version and its JS runtime. Used by the media downloader, video transcript, and thumbnail tools. | Implemented on Linux |
| Images | libvips 8.14+ | Reuse a compatible system `vips`/`vipsheader` pair; probe the installed loaders and savers (PNG, JPEG, WebP, TIFF, HEIF/AVIF, GIF, BMP). | Implemented on Linux |
| Image composition | ImageMagick 7 (`magick`) | Combine, watermark, favicon, passport photo, and PDF signature images. Staged inputs use fixed names and resource limits; user text cannot read files or expand escapes. | Implemented on Linux |
| OCR | Tesseract | Reuse the engine and its installed language packs; local only. | Implemented on Linux |
| PDF structural edits | qpdf 11.9+ | Merge, split, organize, overlay (watermark, signature), protect/unlock, and lossless optimization. JPEG recompression is offered only when `--optimize-images` and `--jpeg-quality` are detected. | Implemented on Linux |
| PDF rendering and extraction | Poppler (`pdftoppm`, `pdftotext`, `pdfinfo`, `pdfimages`, `pdfdetach`) | Page images, text, metadata, embedded images, and attachments. | Implemented on Linux |
| PDF size targets | Ghostscript (`gs`) | Resamples images down a DPI ladder until the PDF fits the requested size. | Implemented on Linux |
| Images to PDF | img2pdf | Lossless JPEG/PNG embedding. Install with `uv tool install img2pdf`. | Implemented on Linux (img2pdf 0.6.3) |
| Searchable PDF | OCRmyPDF + Tesseract + Ghostscript | Install with `uv tool install ocrmypdf`. Pages that already have text are skipped. | Implemented on Linux (OCRmyPDF 17.13) |
| Office rendering | LibreOffice (`soffice`) | Detect an installed suite; never bundle it. | Conditional: not installed in the test environment |
| Webpage capture | Chromium / Chrome | Driven over the DevTools protocol with a throwaway profile for PDF and full-page screenshots. | Implemented on Linux |
| Network checks | curl, system `ping` | Site check, page images, file downloads. No third-party status API. | Implemented on Linux |
| Speech recognition | Groq `whisper-large-v3` (cloud) | Transcription, translation to English, auto-subtitles, and the transcript fallback. Needs `GROQ_API_KEY` (see below). Audio is sent to Groq; tools are labelled `CLOUD`. | Implemented |
| Text to speech | Piper + `.onnx` voices | Reuse `piper` from `PATH` or `<app data>/tools/piper`; voices from `<app data>/models/piper-voices`. | Implemented on Linux |
| Noise removal | FFmpeg `arnndn` + RNNoise model | Speech mode needs `<app data>/models/rnnoise/sh.rnnn`; general mode uses `afftdn` and needs no model. | Implemented on Linux |
| Vocal separation | Demucs (optional) | Uses `demucs` when installed; otherwise a basic centre-channel FFmpeg filter. | Basic method implemented; Demucs untested |
| Background removal | rembg CLI + cached U²-Net / U²-Net-P model | Reuse an existing local CLI and cached ONNX model in custom local-model mode with an explicit cache root. Never permit first-use model download. | Implemented on Linux |
| Image upscaling | Real-ESRGAN NCNN/Vulkan | Reuse an executable from `PATH` or `<app data>/tools/realesrgan` with its x4plus `.param`/`.bin` pair. | Implemented on Linux |

`<app data>` is the platform application data folder (`~/.local/share/dev.arcadebox.app` on Linux). Discovery also searches `~/.local/bin` and each folder under `<app data>/tools`.

## Groq API key

Speech tools read `GROQ_API_KEY` from the process environment, then from `<app data>/.env`. Debug builds also read the repository's `.env`, which is git-ignored. The key is sent to curl through a private header file, never in arguments, logs, or results.

## Local model assets

The model registry separates model files from provider executables. `ModelAssetInfo` records the stable model ID and name, optional version/hash, local path, source, license, provenance, and compatible provider. A matching executable is reported separately. Provider discovery accepts only already-present model files; none of the image model handlers can fetch weights as a side effect.

The current rembg cache model assets and Real-ESRGAN weight files do not have license evidence pinned by Arcade Box. Their registry records say so explicitly. These are user-managed local assets; Arcade Box does not redistribute them. A managed model installer must wait until Arcade Box can pin a source, version, SHA-256, license text, and required notices for each exact model package.

Minimum compatible version/capability, platform coverage, managed availability, and license must be recorded here before a capability ships. Do not claim a capability from version-string comparison alone; test the operation that the tool needs. Provider health appears in Settings with source, path, version, compatible capabilities, and a concise reason when rejected.

## Provider provenance

Persist provider ID, resolved path/package source, version, verification result, capabilities, and check time. Recheck when the binary changes or a job needs a capability not covered by the cached probe. Never silently update a system provider. A managed provider download is integrity-checked and its license/source/size shown before install.
