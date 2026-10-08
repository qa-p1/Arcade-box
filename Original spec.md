# ARCADE BOX

> Original product brief; aspirational features are not implementation claims. Current implementation and limits: [status](docs/STATUS.md).

## Master Product, Architecture, UX, Engineering, Plugin SDK, Tool Catalog, and Implementation Specification


# 1. PRODUCT DEFINITION

Arcade Box is an **open-source, cross-platform desktop utility platform** intended to replace the scattered collection of:

* random utility websites
* tiny one-purpose applications
* browser extensions
* PowerToys-style utilities
* media converters
* PDF websites
* image tools
* online encoders/decoders
* QR websites
* media downloaders
* screenshot utilities
* developer utility websites
* file utilities
* network diagnostic sites
* calculator/conversion websites
* clipboard tools
* system mini-utilities

that users currently need for small everyday tasks.

The product must not feel like:

> “120 mini-applications dumped into one dashboard.”

The product should feel like:

> **one universal interface through which the user can instantly perform hundreds of actions.**

The defining workflow is:

**invoke → type → select → perform → disappear**

A user should be able to think:

> “compress this PDF”

and be using the correct tool seconds later.

---

# 2. CORE PRODUCT PRINCIPLE

Arcade Box is not primarily a launcher.

Arcade Box is not primarily a dashboard.

Arcade Box is not primarily a toolbox.

It is a **universal action surface for desktop computing**.

The normal interaction should be:

1. Press the global Arcade Box shortcut.
2. A polished compact interface drops from the top-center of the active display.
3. Start typing immediately.
4. Search resolves tools and relevant actions.
5. Press Enter.
6. The interface smoothly expands into that tool.
7. Perform the task.
8. Receive the result.
9. Continue the result into another tool if desired.
10. Press Escape.
11. Arcade Box disappears and returns focus to the previous application.

The dashboard is important, but primarily for:

* discovery
* browsing categories
* managing tools
* favorites
* history
* pipelines
* plugin installation
* settings
* dependencies
* documentation

Frequent users should barely need it.

---

# 3. OPEN-SOURCE REQUIREMENT

Arcade Box is completely open source and hosted publicly on GitHub.

The repository must contain enough source and documentation for another developer to:

* audit Arcade Box
* build it
* contribute tools
* develop plugins
* understand the architecture
* package it
* reproduce releases where practical

Perform a formal dependency-license audit early in the project.

Do not accidentally choose dependencies whose distribution requirements conflict with the chosen project license or each other.

A copyleft license such as **GPL-3.0-or-later** is a reasonable starting candidate for the main application, but verify all implications before permanently committing to the license.

Keep a machine-readable third-party license inventory.

Generated distributions must include required license notices.

---

# 4. SUPPORTED OPERATING SYSTEMS

Treat all three major desktop platforms as first-class from the beginning:

### Windows

Primary target:

* Windows 11

Support appropriate current architectures where practical:

* x86-64
* ARM64

### macOS

Support currently maintained macOS versions.

Prioritize:

* Apple Silicon
* Intel where practical

### Linux

Linux is not an afterthought.

Primary modern target:

* Wayland

Also support:

* X11 where reasonably possible

Do not develop a Windows application and leave portability to later.

Platform-specific capabilities must be implemented behind explicit platform abstractions from the beginning.

A feature may legitimately require different native implementations on Windows, macOS, Wayland, and X11.

That is preferable to pretending the platforms behave identically.

---

# 5. LINUX REQUIREMENTS

For Wayland, prefer standards-based integrations.

Use XDG Desktop Portals where appropriate for things such as:

* global shortcuts
* screenshots
* screencasting
* file selection
* opening files
* URI handling

Use PipeWire for modern Linux screen/video capture where applicable.

Do not solve Wayland integration by relying entirely on compositor-specific hacks.

Compositor-specific enhancements may exist as optional adapters, but portable portal-based behavior should remain the baseline.

Gracefully detect capability availability.

If a desktop environment does not expose a required portal or protocol, explain the limitation clearly rather than silently failing.

---

# 6. PRIMARY TECHNOLOGY DIRECTION

Start from the following architecture unless investigation uncovers a materially better solution.

## Native core

Use **Rust** for the main backend/core.

Rust should own things such as:

* tool runtime
* tool registry
* provider resolution
* process execution
* pipeline execution
* job management
* platform abstraction
* file operations
* search indexing
* permissions
* plugin host
* networking
* updates
* configuration
* sensitive-data handling

Avoid sending large media buffers through frontend IPC unnecessarily.

## Desktop framework

Use **Tauri 2** as the primary desktop shell unless an evidence-based architectural review demonstrates a superior alternative.

Take advantage of Tauri's capability/permission model instead of exposing broad backend access to the frontend.

Do not grant the frontend unrestricted shell or filesystem access.

## Frontend

Use a high-performance TypeScript frontend.

Strong preference:

* Svelte / current Svelte ecosystem

or another lightweight reactive frontend if investigation proves it superior.

Avoid Electron.

Arcade Box's identity depends partly on:

* fast startup
* low idle overhead
* responsive interactions
* small distribution footprint relative to Electron applications

## Local persistence

Use **SQLite** for application state that benefits from relational/queryable persistence.

Examples:

* usage statistics
* tool history
* favorites
* installed plugin metadata
* pipeline definitions
* dependency state
* search aliases
* user settings

Use migrations from the beginning.

Do not store large working media in SQLite.

---

# 7. THE ARCADE ISLAND

The fast overlay will be referred to internally as the **Arcade Island**.

It is the central interaction surface.

It should visually feel like a premium native desktop component rather than a floating webpage.

Do not directly clone Apple's Dynamic Island visual design.

Develop Arcade Box's own identity around the same useful concept:

> compact surface → animated expansion → contextual action surface.

## Appearance

Default placement:

* top center
* current/active monitor

Characteristics:

* rounded geometry
* subtle depth
* restrained translucency where appropriate
* system-aware dark/light appearance
* excellent typography
* clean iconography
* minimal visual noise
* no thick title bar
* no unnecessary window chrome

It should look intentional on:

* Windows
* macOS
* GNOME
* KDE
* Hyprland
* other modern desktop environments

Do not attempt to mimic one operating system exactly.

## Animation

Animations must communicate state.

Examples:

* compact search capsule expands vertically into results
* selecting a tool morphs results into tool controls
* running jobs collapse into progress status
* results appear as actionable cards
* returning to search reverses naturally

Favor compositor-friendly animation:

* transform
* opacity
* clipping
* scale where appropriate

Avoid heavy layout thrashing.

Respect the OS reduced-motion preference.

Target consistently smooth rendering at the display refresh rate where practical.

---

# 8. GLOBAL SHORTCUT

Arcade Box must have a global shortcut.

Do not force one universal shortcut that conflicts badly with OS conventions.

During first-run onboarding:

1. propose a sensible platform-specific default
2. test whether it can be registered
3. detect conflicts
4. allow immediate customization

Users must always be able to change it later.

The launcher must appear on the currently relevant display.

Closing it should restore focus cleanly to the previously active application whenever the platform permits.

---

# 9. SEARCH EXPERIENCE

Search is one of the most important pieces of the product.

Search must be effectively instant.

Typing:

> pdf merge

should find:

> Merge PDFs

Typing:

> join document

should also find it.

Typing:

> make jpg smaller

should strongly suggest:

> Compress Image
> Resize Image

Typing:

> video audio

should strongly suggest:

> Extract Audio from Video

Typing:

> qr screen

should strongly suggest:

> Scan QR/Barcode from Screen

Every tool manifest must therefore include curated metadata such as:

* canonical name
* short name
* category
* description
* aliases
* verbs
* nouns
* accepted inputs
* produced outputs
* related tools
* common natural phrases

Search ranking should combine:

* direct match
* fuzzy match
* alias match
* token relevance
* action/object intent
* current context
* recently used tools
* favorites
* frequency
* file type
* clipboard type

Usage history must improve ranking without making infrequently used exact matches impossible to reach.

Search must not require an internet LLM.

Optional future semantic/NL features may enhance it, but the product must remain excellent offline.

---

# 10. CONTEXT-AWARE ACTIONS

Arcade Box should understand relevant context when invoked.

Examples:

Clipboard contains JSON:

* Format JSON
* Validate structured data
* JSON → YAML
* Copy normalized JSON

Clipboard contains a URL:

* Download media
* Resolve redirect
* Generate QR
* Download file
* Inspect page
* Save webpage

Clipboard contains an image:

* Compress
* Resize
* OCR
* Scan QR/barcode
* Remove metadata
* Extract palette

Clipboard contains text:

* Clean text
* Translate
* Change case
* Compare
* Encode
* Generate QR

This behavior must be privacy-conscious.

Do not continuously upload or remotely analyze the clipboard.

Default behavior should inspect context locally when Arcade Box is invoked.

Clipboard history is a separate explicit feature with its own privacy controls.

---

# 11. SCREEN SELECTION MODE

Provide a dedicated screen-action shortcut.

When triggered:

1. dim the display
2. allow the user to select a rectangle/window/screen as supported
3. capture the selection
4. inspect it locally
5. show relevant actions inside Arcade Island

Example selection containing text:

* Copy text
* OCR
* Translate
* Search
* Save image

Example QR selection:

* decoded content
* Copy
* Open
* Save
* Inspect

Example ordinary image:

* Copy
* Save
* Crop
* Compress
* OCR
* Scan barcode
* Extract colors
* Redact
* Send to another tool

---

# 12. DRAG AND DROP

Users should be able to drag files directly onto:

* the Arcade Island
* tool interfaces
* dashboard tool cards where sensible

When files are dropped onto the empty island, infer compatible tools from MIME type/content.

Dropping a PDF could immediately suggest:

* Compress
* Split
* OCR
* Extract
* Protect

Dropping an MP4 could suggest:

* Trim
* Convert
* Compress
* Extract audio
* Inspect streams

Use content/magic detection rather than trusting filename extensions alone.

---

# 13. UNIVERSAL RESULT MODEL

Every tool should return structured results instead of treating completion as merely “a subprocess exited.”

A result can include:

* output files
* text
* URLs
* images
* media
* structured JSON
* metadata
* warnings
* statistics

The UI should expose contextual actions:

* Open
* Reveal in folder
* Copy
* Save as
* Replace original
* Send to another tool
* Add to pipeline
* Repeat
* View details

Default behavior should preserve original files.

Destructive overwrite must be explicit.

---

# 14. TYPED TOOL I/O

This is foundational.

Every tool declares accepted input and output types.

Examples:

* `text/plain`
* `text/url`
* `structured/json`
* `file/pdf`
* `file/image`
* `file/video`
* `file/audio`
* `screen/region`
* `clipboard/image`
* `network/url`

Do not pass multi-gigabyte media between pipeline stages as serialized IPC data.

Use references/handles/temporary artifacts.

Typed I/O powers:

* search relevance
* context suggestions
* pipelines
* compatibility checks
* plugin safety
* automatic “next action” suggestions

---

# 15. PIPELINE ENGINE

Pipelines are a first-class architectural feature, not a later hack.

Example:

URL
→ Download Media
→ Extract Audio
→ Normalize Loudness
→ Write Metadata
→ Save

Example:

Screen Region
→ OCR
→ Translate
→ Copy

Example:

Images
→ Resize
→ Remove Metadata
→ Compress
→ PDF

Internally design the engine as a DAG-capable execution system even if the first public editor makes common linear workflows easiest.

Support:

* typed connections
* branching
* reusable variables
* file references
* progress
* cancellation
* retry
* errors
* intermediate artifacts
* cleanup
* conditional stages where useful
* batch inputs

Do not force automation into tools that do not benefit from it.

The purpose of pipelines is **ease and speed**, not visual-programming complexity.

Users must be able to save pipelines under friendly names.

Saved pipeline:

> Clean Audio

could become searchable exactly like a normal tool.

Eventually allow assigning a dedicated shortcut to a pipeline.

---

# 16. BACKGROUND JOBS

A long operation must not force Arcade Island to remain open.

Example:

* compressing 8 GB video
* downloading a playlist the user is authorized to download
* OCRing 700 pages
* upscaling images

The user should be able to close the Island.

A lightweight job system continues work.

The tray/dashboard shows progress.

Completion may produce an OS notification if enabled.

Heavy worker processes should exist only while required.

Do not permanently run FFmpeg, OCR engines, Python, browsers, etc.

---

# 17. PUBLIC PLUGIN SYSTEM FROM DAY ONE

Do not make first-party tools use one internal architecture while community plugins later get a second inferior API.

That would create a future rewrite.

From the beginning, define a real **Tool SDK**.

First-party tools should use the same fundamental contracts available to external developers.

## Tool manifest

Each tool should declare information comparable to:

```yaml
id: arcade.pdf.merge
version: 1.0.0

name: Merge PDFs
category: documents

aliases:
  - join pdf
  - combine pdfs
  - pdf merger

inputs:
  - file/pdf[]

outputs:
  - file/pdf

execution:
  runtime: wasm

permissions:
  filesystem:
    read: user-selected
    write: user-selected

network: none

providers: []

pipeline:
  batch: true
  deterministic: true
```

The actual schema may differ, but it must cover:

* stable ID
* version
* API version
* name
* description
* category
* aliases
* icon
* OS support
* architecture support
* inputs
* outputs
* permissions
* network access
* provider requirements
* configuration
* batch support
* pipeline support
* UI definition
* execution backend
* side effects

Use semantic versioning.

---

# 18. PLUGIN EXECUTION SECURITY

Do **not** load arbitrary third-party native libraries directly into the Arcade Box core process.

Prefer sandboxable plugin execution.

Strong architectural direction:

* WebAssembly
* WASI
* Component Model / WIT interfaces using the stable ecosystem available at implementation time
* Wasmtime or an equivalent mature runtime

The host exposes explicit capabilities.

Plugins should request operations such as:

> read this user-selected file

rather than obtaining unrestricted filesystem access.

Where advanced native functionality is necessary, use a separately governed **provider** or extension model with stronger trust requirements.

---

# 19. ADVANCED PLUGIN UI

Most tools should be expressible using Arcade Box's standard controls:

* file picker
* text input
* select
* toggle
* slider
* color input
* range
* preview
* sortable list
* result card
* progress bar

This ensures visual consistency.

For truly advanced plugin UIs, allow a sandboxed extension view.

It must:

* run isolated
* use strict CSP
* have no raw Tauri bridge
* communicate through a scoped Arcade API
* receive only declared permissions

---

# 20. PLUGIN REGISTRY

Design for an official public Arcade Box registry.

A package should have:

* author
* source repository
* semantic version
* compatibility requirements
* manifest
* package hash
* signature
* permissions
* license
* changelog

The application should clearly show required permissions before installation.

Example:

> **Network Inspector**
>
> Requires:
>
> * network access
> * read user-selected files
>
> Does NOT require:
>
> * home-folder access
> * clipboard history
> * process execution

Support:

* install
* update
* uninstall
* disable
* rollback

Do not let a malicious registry update silently gain new permissions.

Permission escalation requires user acknowledgement.

---

# 21. PROVIDER ARCHITECTURE

Many tools should use mature external projects rather than reimplementing complex codecs and formats.

Introduce a **Provider Registry/Broker**.

Examples:

* FFmpeg provider
* yt-dlp provider
* OCR provider
* PDF provider
* archive provider
* document-conversion provider
* metadata provider
* ML model provider

Tools depend on capabilities, not hardcoded executable paths.

Example:

`video.compress`

requests:

`media.ffmpeg >= required-capabilities`

The provider system determines how to satisfy it.

---

# 22. SYSTEM DEPENDENCIES MUST BE REUSED

This is non-negotiable.

If a compatible tool is already installed globally, Arcade Box should normally use it rather than downloading another copy.

This particularly applies to things such as:

* `ffmpeg`
* `ffprobe`
* `yt-dlp`
* `tesseract`
* `7z`
* `qpdf`
* `libreoffice`
* `exiftool`
* suitable JS runtime required by current yt-dlp
* other supported providers

At startup or on-demand, probe:

* `PATH`
* common application locations
* platform package locations
* user-configured binary paths

Do not merely check whether the filename exists.

Verify:

* executable identity
* version
* required capability
* basic health

Example:

> FFmpeg
> System installation detected
> `/usr/bin/ffmpeg`
> Version: …
> Status: Compatible
> **Using system provider**

If the global installation is incompatible, explain why.

Then offer an Arcade-managed provider.

Never silently overwrite/update the user's system installation.

---

# 23. ARCADE-MANAGED PROVIDERS

If a required dependency is missing, Arcade Box may offer:

> Install provider

Show:

* dependency name
* purpose
* download size
* license
* source
* installed version

Arcade-managed providers should live inside Arcade Box's own application data.

Do not pollute the user's global environment.

Verify downloaded providers cryptographically.

Allow removing managed providers individually.

---

# 24. MEDIA STACK

Use mature upstream technology.

For general audio/video:

* FFmpeg
* ffprobe

Use FFmpeg for appropriate operations such as:

* transcoding
* remuxing
* trimming
* filtering
* resizing
* extraction
* audio normalization
* GIF/video processing

Do not reinvent media codecs.

Construct subprocess calls using structured argument arrays.

Never build shell commands by concatenating untrusted filenames.

Use ffprobe's machine-readable output to build the media-inspection model.

---

# 25. MEDIA DOWNLOAD STACK

Use **yt-dlp** as the primary general media acquisition engine where appropriate.

Do not build and maintain hundreds of fragile site extractors ourselves when yt-dlp already solves this problem.

Architecture:

Arcade Download Tool
→ downloader abstraction
→ yt-dlp provider
→ FFmpeg provider when post-processing is required

The UI should not expose raw yt-dlp complexity by default.

Provide a clean experience:

URL

Then useful options:

* best quality
* quality selection
* video
* audio only
* output format
* subtitles
* metadata
* thumbnail
* filename
* destination

Advanced options can expose more control.

Detect and use the current yt-dlp requirements for full platform support, including its current JavaScript execution support when required.

Use a globally installed compatible runtime when possible.

Do not bundle another runtime unnecessarily.

### Safety/legal behavior

Arcade Box must not be designed to defeat:

* DRM
* paywalls
* access controls
* private-content restrictions

Support:

* public downloadable content
* content the user owns
* content the user is otherwise authorized to download

Authenticated support, if implemented, must be explicit and securely handled.

Never casually expose browser cookies to plugins.

---

# 26. IMAGE STACK

Prefer **libvips** for mainstream image processing because the application needs:

* low memory usage
* streaming-friendly behavior
* excellent performance
* broad format support

Use additional specialized libraries only where they provide clear benefits.

Avoid decoding every huge image into multiple unnecessary full-resolution copies.

For ML-based tools such as:

* background removal
* upscale

use an optional local model-provider system.

Do not make core Arcade Box depend on multi-gigabyte ML packages.

Model packages should be:

* optional
* versioned
* licensed correctly
* downloadable independently
* hardware accelerated when available
* usable with CPU fallback when realistically possible

---

# 27. OCR

Provide a provider abstraction.

A robust baseline should use a mature local OCR engine such as Tesseract.

Native platform OCR can optionally be used where it demonstrably improves quality/performance.

The UI should expose language-pack availability cleanly.

Language packs should be separately installable.

Do not require cloud OCR for ordinary OCR.

---

# 28. QR AND BARCODE

Use a mature multi-format implementation such as **ZXing-C++** behind the barcode provider.

Support common formats without maintaining custom QR decoding logic.

Barcode handling should support both creation and reading where the underlying format permits.

---

# 29. ARCHIVES

Prefer an established archive implementation such as:

* libarchive
* 7-Zip provider where appropriate

Support common archive formats.

Do not implement ZIP/7z/TAR parsers from scratch unless a specific library requirement demands it.

---

# 30. PDF STACK

PDF functionality will likely require several complementary components.

Do not search for one magical PDF library and force every operation through it.

Separate capabilities:

* structural editing
* page manipulation
* encryption
* rendering
* text extraction
* OCR
* optimization

Use mature libraries/providers such as qpdf and a suitable open-source rendering engine after validating current licensing and distribution requirements.

Avoid a licensing choice that contaminates distribution unintentionally.

---

# 31. OFFICE DOCUMENT CONVERSION

Do not bundle an enormous office suite inside Arcade Box merely to check a box.

For formats requiring office rendering, detect a compatible installed **LibreOffice** installation and expose it as a provider.

If absent:

* support formats Arcade can convert correctly itself
* explain that Office conversion requires an optional provider
* offer clear setup guidance or an optional provider strategy if distribution is reasonable

Never claim high-fidelity DOCX/XLSX/PPTX conversion when the backend cannot actually preserve it.

---

# 32. LOCAL / NETWORK / CLOUD CLASSIFICATION

Every tool must declare one of the following execution/privacy classes.

## LOCAL

No outbound network required for the operation.

Example:

* resize image
* merge PDF
* calculate hash

UI indicator:

**LOCAL**

## NETWORK

Network access is inherent to the tool.

Example:

* ping
* media downloader
* DNS lookup
* currency lookup

UI indicator:

**NETWORK**

## CLOUD

User content may be processed by a remote third-party service.

Example:

* optional cloud translation provider

UI indicator:

**CLOUD**

Cloud tools must clearly state what information leaves the device.

No file should be uploaded merely because cloud functionality exists.

---

# 33. THE 120 INITIAL TOOLS

The following are **actual distinct tools**, not a requirement to create 120 dashboard cards by artificially splitting every checkbox into another tool.

Modes that logically belong together should remain together.

All 120 must have:

* real implementation
* polished UI
* error handling
* documentation
* search aliases
* typed I/O
* platform status
* permission metadata
* tests appropriate to the tool
* pipeline compatibility where meaningful

Do not ship fake placeholder tools.

---

# CATEGORY 1 — PDF & DOCUMENTS

### 1. Merge PDFs

Combine multiple PDFs.

Features:

* drag reorder
* preview pages
* batch files
* preserve bookmarks where reasonably possible
* output filename selection

### 2. Split / Extract PDF Pages

Support:

* range syntax
* selected pages
* every N pages
* one file per page

### 3. PDF Page Organizer

Visual page grid supporting:

* reorder
* rotate
* delete
* duplicate
* multi-select

### 4. Compress / Optimize PDF

Useful modes:

* lossless structural optimization
* balanced
* small file
* custom image-resolution/compression controls

Always explain when compression may be lossy.

### 5. Images → PDF

Support:

* multiple images
* drag ordering
* page size
* fit/fill
* margin
* orientation
* DPI handling

### 6. PDF → Images

Support common outputs:

* PNG
* JPEG
* WebP where appropriate

Controls:

* page selection
* scale/DPI
* quality

### 7. Searchable PDF OCR

Input scanned PDF.

Produce:

* original-looking PDF
* searchable/selectable OCR layer

Support language selection.

### 8. PDF Content Extractor

One coherent tool for extracting:

* text
* tables where reliably detectable
* images
* attachments
* metadata

Do not claim perfect table reconstruction.

### 9. PDF Watermark / Stamp / Page Numbers

Support:

* text watermark
* image watermark
* header/footer
* page numbers
* opacity
* position
* page range

### 10. PDF Protect / Unlock

Support:

* set encryption/password
* remove encryption when the user provides the valid password
* supported permission controls

Do not provide password cracking.

### 11. Document → PDF

Support formats Arcade can accurately render.

Use installed LibreOffice provider for office formats when needed.

---

# CATEGORY 2 — IMAGES & GRAPHICS

### 12. Image Format Converter

Common formats including, as provider support allows:

* PNG
* JPEG
* WebP
* AVIF
* HEIC
* TIFF
* BMP
* GIF

Preserve transparency when possible.

### 13. Image Resize / Resample

Support:

* dimensions
* percentage
* fit
* fill
* longest edge
* shortest edge
* aspect lock
* high-quality resampling

### 14. Crop / Rotate / Flip

Fast visual editor.

### 15. Image Compressor / Optimizer

Modes:

* quality target
* size target
* lossless
* lossy

Show before/after:

* file size
* dimensions
* percentage reduction

### 16. Background Remover

Local model where practical.

Provide transparent PNG/WebP output.

Use a separately managed model package.

### 17. Image Upscale / Enhance

Local model provider.

Useful scale choices.

Do not silently invent detail without identifying that AI enhancement is being used.

### 18. Image Metadata Inspector & Sanitizer

View metadata.

Allow:

* remove all nonessential metadata
* remove location
* edit supported metadata
* preserve orientation correctly

### 19. Blur / Pixelate / Redact

Visual selectable regions.

Redaction mode should actually remove/replace pixel information rather than merely placing a reversible overlay.

### 20. Palette / Dominant Color Extractor

Output:

* swatches
* HEX
* RGB
* HSL

Copy individual values.

### 21. Image Compare / Diff

Modes:

* side by side
* overlay
* slider
* pixel difference
* difference statistics

### 22. Image OCR → Text

Input:

* image file
* clipboard image

Output selectable/copyable text.

---

# CATEGORY 3 — VIDEO

### 23. Video Convert / Transcode

Clean presets plus advanced mode.

Expose sensible containers/codecs based on installed FFmpeg capabilities.

### 24. Video Trim / Cut

Fast visual timeline.

Use stream copy when technically valid and accurate.

Re-encode when required.

Explain distinction only when relevant.

### 25. Video Crop / Resize / Rotate

Visual preview.

Useful aspect presets.

### 26. Video Compress / Target Size

Modes:

* quality-based
* target size
* resolution-based

Estimate output before processing where possible.

### 27. Join / Merge Video Clips

Validate codec/container compatibility.

Use remux without re-encoding where possible.

Fallback to controlled transcoding.

### 28. Extract Audio from Video

Offer useful audio format/quality choices.

### 29. Video → GIF / Animated WebP

Controls:

* start/end
* FPS
* size
* loop
* quality

Optimize palette for GIF.

### 30. Extract Video Frames

Support:

* single timestamp
* every N seconds
* every N frames
* selected range

### 31. Video / Media Inspector

Use ffprobe.

Display:

* container
* streams
* codec
* resolution
* frame rate
* color information
* HDR information where available
* audio channels
* bitrate
* duration
* subtitles
* chapters
* metadata

Allow raw technical output in Advanced view.

### 32. Subtitle Tool

Support appropriate combinations of:

* extract subtitle streams
* add external subtitle track
* remove subtitle track
* convert subtitle formats
* burn subtitles into video

---

# CATEGORY 4 — AUDIO

### 33. Audio Format Converter

Common audio codecs/containers through FFmpeg.

### 34. Audio Trim / Cut

Waveform view.

Accurate time selection.

### 35. Join / Merge Audio

Support compatible joining and conversion where needed.

### 36. Loudness Normalizer

Use modern loudness measurement/normalization.

Expose practical presets rather than meaningless “volume +200%”.

Show measured values where useful.

### 37. Speed / Pitch Tool

Allow:

* speed while preserving pitch
* pitch adjustment
* combined manipulation

### 38. Silence Trim / Split

Detect silence.

Use cases:

* remove leading/trailing silence
* split recording at silence
* configurable threshold/duration

### 39. Audio Metadata / Cover Editor

Support common metadata:

* title
* artist
* album
* track
* disc
* year
* genre
* cover art

Preserve existing tags when not changed.

---

# CATEGORY 5 — WEB & DOWNLOADS

### 40. Universal Media Downloader

Primary yt-dlp frontend.

Support platforms supported by the installed yt-dlp provider.

One tool, not one tool per website.

Useful options:

* video/audio
* quality
* format
* subtitles
* thumbnail
* metadata
* destination

### 41. Direct File Downloader

HTTP/HTTPS download utility.

Support where servers permit:

* resume
* progress
* redirects
* checksums when supplied
* filename detection

### 42. Webpage Snapshot

Save a webpage as one or more supported representations:

* PDF
* image
* MHTML/archival form where backend permits

Do not bundle an entire browser unless justified.

Use native webview or optional headless-browser provider as appropriate.

### 43. Webpage → Markdown / Text

Extract readable content.

Preserve useful structure:

* headings
* links
* lists
* code

### 44. Webpage Asset Extractor

Show downloadable assets detected from a user-supplied page:

* images
* media
* documents

Respect authentication and access limitations.

### 45. URL Redirect Resolver

Show complete redirect chain.

Detect:

* shortened URL
* status codes
* final destination

Never automatically open suspicious redirects without user action.

### 46. HLS / DASH Inspector & Downloader

Inspect public/non-DRM manifests.

Show:

* variants
* resolutions
* codecs
* audio tracks

Allow downloading content when the user has permission and the stream is not protected by DRM.

---

# CATEGORY 6 — SCREEN & CAPTURE

### 47. Screenshot

Support where platform permits:

* region
* window
* active window
* display
* all displays

Immediate result actions:

* copy
* save
* annotate
* pin
* OCR
* QR scan

### 48. Screen Recorder

Support:

* display
* region
* window where platform permits
* system audio where platform APIs allow
* microphone
* frame rate
* output quality

Use native capture pipelines and FFmpeg appropriately.

### 49. Screen OCR

Select region.

Extract text.

Copy immediately or send to another tool.

### 50. Screen QR / Barcode Scanner

Select an area.

Decode locally.

Show decoded content safely before acting on it.

### 51. Screen Color Picker

Magnified pixel view.

Output:

* HEX
* RGB
* HSL
* alpha where relevant

### 52. Screen Ruler / Measurement Tool

Measure:

* pixels
* width/height
* distances

Useful magnification/snapping behavior.

### 53. Pin / Freeze Screenshot

Capture a region/window and keep it floating above other applications.

Support multiple pinned references.

---

# CATEGORY 7 — QR & BARCODES

### 54. QR Generator

Input presets:

* plain text
* URL
* Wi-Fi
* contact/vCard
* email
* phone
* SMS

Controls:

* error correction
* size
* margin
* foreground/background

Export:

* PNG
* SVG

### 55. Barcode Generator

Support useful supported symbologies such as:

* Code 128
* Code 39
* EAN
* UPC
* Data Matrix
* PDF417
* Aztec

Only expose formats correctly supported by provider.

### 56. QR / Barcode Decoder

Input:

* image file
* clipboard image
* camera if supported and permission granted

Display raw decoded data before launching links.

### 57. Batch Barcode / Label Generator

Input structured rows from:

* CSV
* pasted table
* manual list

Create printable/exportable label sheets.

---

# CATEGORY 8 — TEXT & DATA

### 58. Text Diff / Merge

Support:

* side-by-side
* inline
* copied text
* files
* optional three-way merge

### 59. Case & Typography Converter

Useful transformations such as:

* uppercase
* lowercase
* title case
* sentence case
* camelCase
* PascalCase
* snake_case
* kebab-case

### 60. Whitespace / Line Cleaner

Actions such as:

* trim lines
* collapse whitespace
* remove blank lines
* normalize line endings
* normalize tabs/spaces

### 61. Sort / Deduplicate / Shuffle Lines

Support:

* lexical
* numeric
* natural
* ascending/descending
* case sensitivity
* unique
* random shuffle

### 62. Structured Data Formatter / Validator

One coherent workbench for formats such as:

* JSON
* YAML
* TOML
* XML

Support:

* prettify
* compact
* validation
* parse errors with location

### 63. Structured Data Converter

Convert where semantically valid between:

* JSON
* YAML
* TOML
* XML

Warn when the conversion cannot be perfectly lossless.

### 64. CSV / TSV Workbench

Support:

* inspect
* delimiter detection
* sort
* filter
* deduplicate
* select columns
* rename columns
* convert CSV/TSV/JSON

Handle large files without freezing the UI.

### 65. Markdown Preview / Export

Live preview.

Useful export:

* HTML
* PDF through appropriate renderer

### 66. Unicode / Character Inspector

Inspect characters:

* Unicode code point
* UTF-8 representation
* Unicode name
* whitespace/control status

Useful for debugging invisible characters.

### 67. Text Statistics / Readability

Display:

* characters
* words
* lines
* sentences
* estimated reading time
* useful readability metrics

### 68. Translate Text

Provider-based.

Prefer local provider where available.

Allow configurable cloud translation providers.

Clearly label CLOUD execution when remote.

---

# CATEGORY 9 — DEVELOPER UTILITIES

### 69. Regex Playground

Support:

* expression
* flags
* test text
* highlighted matches
* capture groups
* replacement preview

Use a capable mature regex engine.

Clearly identify engine semantics.

### 70. Base / Binary Encoding Tool

Support relevant encodings:

* Base64
* Base32
* Hex
* binary representation

Encode/decode text/files where appropriate.

### 71. URL Encode / Decode / Query Inspector

Support:

* percent encode/decode
* query parser
* query editing
* rebuilt URL

### 72. Hash / Checksum Tool

Support modern hashes:

* SHA-256
* SHA-512
* BLAKE3

Allow legacy hashes such as MD5 only for compatibility/checksum workflows and label them unsuitable for modern security.

Input:

* text
* file

### 73. UUID / ULID / Identifier Generator

Support:

* UUID variants where appropriate
* ULID
* bulk generation

### 74. JWT Inspector

Decode locally.

Show:

* header
* payload
* timestamps
* algorithm

Optionally verify signature when the user supplies appropriate verification material.

Never upload tokens to an external service by default.

### 75. Epoch / ISO Date Converter

Convert between:

* Unix time
* milliseconds
* ISO-8601
* local time
* UTC

### 76. Cron Builder / Explainer

Parse common cron syntax.

Show human-readable schedule and next occurrences.

Clearly distinguish cron dialect differences.

### 77. SQL Formatter

Format SQL.

Allow configurable dialect where supported.

No database connection required.

### 78. SemVer / Version Range Tester

Compare versions.

Test whether versions satisfy ranges.

Useful for package-development workflows.

### 79. Code Formatter / Minifier

Provide supported formatters through safe provider adapters.

Useful formats may include:

* HTML
* CSS
* JavaScript
* TypeScript

Do not invent a half-correct formatter when a mature formatter provider can be used.

---

# CATEGORY 10 — FILES & ARCHIVES

### 80. Archive Create / Extract

Support common formats through mature providers.

Actions:

* create
* extract
* inspect

### 81. Batch Rename

Powerful preview before applying.

Rules:

* replace
* regex
* numbering
* prefix/suffix
* case
* date
* metadata where supported

Never apply rename without showing the resulting names.

### 82. Duplicate File Finder

Efficient staged approach:

* size
* fast fingerprint
* cryptographic/full hash when necessary

Allow review before deletion.

### 83. File / Folder Compare

Compare:

* files
* folder trees

Show:

* only left
* only right
* changed
* identical

### 84. File Split / Join

Split huge files into parts.

Join them reliably.

Create a manifest/checksum where useful.

### 85. Disk / Folder Size Analyzer

Fast tree analysis.

Show largest:

* directories
* files
* extensions

Do not require elevated permissions unless requested.

### 86. File Type / Magic Inspector

Identify actual format independent of extension.

Show useful headers and MIME information.

### 87. Filename Sanitizer

Normalize invalid/problematic filenames for target platforms.

Modes:

* Windows-safe
* macOS-safe
* Linux-safe
* portable

Preview changes.

### 88. Directory Tree Exporter

Generate tree representation in:

* plain text
* Markdown
* JSON

Allow depth/filter controls.

---

# CATEGORY 11 — NETWORK & WEB DIAGNOSTICS

### 89. Ping

Simple polished latency/loss UI.

### 90. Traceroute

Cross-platform implementation/provider.

Visualize hops and latency.

### 91. DNS Lookup

Support common record types.

Allow resolver choice.

### 92. RDAP / WHOIS Lookup

Prefer RDAP when available.

Display structured registration information cleanly.

### 93. HTTP Status / Header Inspector

Request URL.

Show:

* response status
* timing
* request/response headers
* redirect information
* compression

### 94. TCP Port Connectivity Test

Test a user-specified host and port.

Report:

* resolved target
* connection success/failure
* latency
* basic error

This is not intended as an aggressive network scanner.

### 95. IP / Subnet Calculator

Support IPv4 and IPv6 as appropriate.

Calculate:

* network
* range
* masks
* CIDR
* address count

### 96. Local / Public IP Inspector

Display:

* local interfaces
* addresses
* gateway/DNS where accessible
* public IP using configurable service

Clearly separate local and externally observed information.

### 97. TLS Certificate Inspector

Given a TLS host/certificate, show:

* subject
* issuer
* SANs
* validity
* algorithm
* chain information where available
* expiry

---

# CATEGORY 12 — SECURITY & PRIVACY

### 98. Password / Passphrase Generator

Cryptographically secure random generation.

Support:

* password
* passphrase
* length
* character groups
* ambiguity controls

### 99. Password Strength Estimator

Entirely local.

Use a realistic strength estimator.

Do not send passwords anywhere.

### 100. File / Text Encryption

Use a mature modern encryption format/provider such as age.

Support:

* encrypt
* decrypt
* passphrase
* recipient key where appropriate

Do not design custom cryptography.

### 101. File Signing / Verification

Use a mature signing system such as minisign or another audited equivalent.

Support:

* sign
* verify
* key information

### 102. Secret Scanner

Inspect user-selected text/files/directories for likely:

* API keys
* credentials
* private keys
* high-risk tokens

Keep scanning local.

Avoid unnecessarily storing matched secrets in logs/history.

### 103. Metadata Privacy Cleaner

Unified privacy tool for supported file formats.

Scan for things such as:

* EXIF location
* author names
* comments
* document properties
* embedded metadata

Show exactly what will be removed.

### 104. PII Redaction Helper

Local-assisted workflow for:

* screenshots
* images
* PDFs

Use OCR/entity heuristics to propose possible sensitive regions.

The user confirms redactions.

Redactions must be destructive in the output pixels/content rather than removable visual overlays.

---

# CATEGORY 13 — CONVERTERS & CALCULATORS

### 105. Calculator

Fast expression calculator.

Support common scientific operations.

Search-bar expressions should work directly.

Example:

`928 * 41`

should produce a result without requiring the user to open the calculator tool first.

### 106. Unit Converter

Broad dimensional conversion:

* length
* mass
* temperature
* area
* volume
* speed
* pressure
* energy
* power
* data
* common additional dimensions

### 107. Currency Converter

Network-backed.

Cache recent rates.

Always show:

* source/provider
* rate timestamp

Do not imply rates are suitable for settlement/trading.

### 108. Time Zone Converter / Meeting Planner

Compare multiple time zones.

Support:

* date
* DST
* copy formatted time

### 109. Date / Duration Calculator

Calculate:

* difference
* add/subtract duration
* business/calendar units where clearly defined

### 110. Data Size / Transfer / Bitrate Calculator

Useful calculations:

* storage units
* transfer time
* network throughput
* media bitrate
* estimated file size

### 111. Aspect Ratio / Resolution Calculator

Calculate:

* missing dimension
* aspect ratios
* scale factors
* common display/video formats

---

# CATEGORY 14 — SYSTEM & DESKTOP

These capabilities enhance the universal command surface without changing Arcade Box into a launcher-first product.

### 112. Application Opener

Search installed applications.

Apps appear after relevant Arcade Box tools in ambiguous searches.

Arcade Box remains tool-first.

### 113. Clipboard History

Explicitly opt-in.

Support:

* text
* images
* file references where possible
* search
* pin
* delete
* clear

Provide privacy controls:

* retention
* exclusions
* sensitive-content handling

### 114. Paste as Plain Text

Global quick action.

Strip formatting safely.

### 115. Window Pin / Always on Top

Where OS/compositor capabilities permit.

Gracefully report unavailable support.

### 116. Process Finder / Terminate

Search processes.

Show:

* name
* PID
* CPU
* memory
* executable where accessible

Require deliberate confirmation for sensitive/privileged termination.

### 117. System Information

Display useful information:

* OS
* version
* CPU
* memory
* GPU
* displays
* storage
* architecture
* network adapters
* application environment

Easy “Copy diagnostics” action.

### 118. Startup Applications Manager

View/manage user startup entries where platform APIs permit.

Do not blindly edit undocumented OS internals.

### 119. PATH / Environment Inspector

Show:

* environment variables
* PATH ordering
* executable resolution
* duplicates
* missing directories

Extremely useful for diagnosing command resolution.

### 120. Default Application / File Association Inspector

Show what application handles common:

* extensions
* MIME types
* URI schemes

Allow changing defaults where the OS provides a supported mechanism.

---

# 34. TOOL UX STANDARDIZATION

A tool should not invent its own entire design language.

Provide reusable Arcade Box components:

* DropZone
* FileList
* SortableFileList
* FilePreview
* ImagePreview
* VideoPreview
* AudioWaveform
* TextEditor
* StructuredEditor
* ToolOptions
* Progress
* ResultCard
* ComparisonView
* BeforeAfterSlider
* Timeline
* RegionSelector
* MetadataTable
* TerminalDetails / AdvancedOutput

Simple tools should require little custom frontend code.

This is important both for first-party velocity and third-party quality.

---

# 35. SIMPLE MODE AND ADVANCED MODE

Do not intimidate normal users with codec flags.

Default UI should answer:

> What does a normal person need?

Example Video Convert:

**Simple**

Format
Quality
Resolution
Output

Then:

> Advanced

can expose:

* codec
* CRF/bitrate
* pixel format
* audio codec
* stream handling
* hardware encoder
* advanced FFmpeg-backed settings

Advanced options must not pollute the default experience.

---

# 36. BATCH PROCESSING

Any tool naturally applicable to multiple items should support batch processing through the shared execution system.

Examples:

* resize 50 images
* convert 30 videos
* sanitize 100 images
* hash 200 files
* OCR several scans

Do not build separate “Batch Image Resize” tools simply to increase tool count.

Batch is a capability of the underlying tool.

---

# 37. TOOL HISTORY

Provide local job history.

Useful information:

* tool used
* time
* success/error
* outputs
* elapsed time

Respect privacy.

Users can configure:

* retain history
* clear automatically
* never store source paths
* clear immediately

Sensitive tools should minimize history by default.

---

# 38. DASHBOARD

The main dashboard should feel premium and calm despite supporting 120+ tools.

Primary sections:

* Search
* Recently Used
* Favorites
* Pipelines
* Categories
* Installed Plugins
* Tool Registry
* Background Jobs

Category pages show clean tool cards.

A tool card should communicate:

* icon
* name
* one-line description
* LOCAL / NETWORK / CLOUD status where useful

Do not fill every card with metadata.

---

# 39. DOCUMENTATION INSIDE THE PRODUCT

Each tool should have concise documentation.

The user should be able to press:

`?`

and understand:

* what the tool does
* accepted input
* output
* privacy
* dependencies
* limitations
* shortcuts
* examples

Documentation should be generated/assembled partly from tool manifests where possible so it cannot drift completely out of sync.

Search should index documentation phrases.

---

# 40. FILE MANAGER INTEGRATION

Provide platform-appropriate **Open with Arcade Box / Arcade Box Actions** integration where feasible.

Example MP4:

* Convert
* Trim
* Compress
* Extract audio
* Inspect media

Example PDF:

* Compress
* Split
* OCR
* Protect

Avoid installing a giant static context menu containing 120 actions.

Resolve compatible actions from input type.

---

# 41. CLI

Arcade Box should also expose a CLI for advanced users and automation.

Possible shape:

```text
arcadebox tools
arcadebox search "compress pdf"
arcadebox run pdf.compress input.pdf
arcadebox run image.resize photo.jpg --width 1920
arcadebox pipeline run clean-audio URL
```

The CLI should invoke the same core runtime.

Do not implement separate backend logic for GUI vs CLI.

Provide shell completions.

---

# 42. DEEP LINKS

Define an application URI scheme.

Example:

```text
arcadebox://tool/pdf.merge
arcadebox://tool/video.trim
arcadebox://pipeline/clean-audio
```

Treat deep-link arguments as untrusted input.

Never allow arbitrary command execution through deep links.

---

# 43. PLATFORM ABSTRACTION LAYER

Define explicit traits/services for things such as:

* GlobalShortcutProvider
* ScreenCaptureProvider
* ScreenRecordingProvider
* ClipboardProvider
* ActiveWindowProvider
* WindowControlProvider
* AppDiscoveryProvider
* ProcessProvider
* AutostartProvider
* CredentialStoreProvider
* FileAssociationProvider
* NotificationProvider
* OpenWithProvider
* PowerStateProvider
* ThemeProvider

Platform implementations belong behind these contracts.

Do not scatter:

```text
if windows
if macos
if linux
```

through every individual tool.

---

# 44. CREDENTIAL STORAGE

Sensitive secrets such as optional API keys must use the platform credential store:

* Windows Credential Manager
* macOS Keychain
* Linux Secret Service where available

Provide reasonable fallback behavior when a secure store is unavailable.

Never store API keys in plain SQLite rows.

---

# 45. PROCESS EXECUTION SECURITY

The provider broker must execute external binaries safely.

Rules:

* no shell interpolation
* structured argument arrays
* strict executable path resolution
* sanitized environment
* controlled working directories
* restricted temporary directories
* cancellation support
* stdout/stderr capture limits
* timeouts where appropriate

Never execute a filename simply because a plugin supplied a string.

---

# 46. TEMPORARY FILE SECURITY

Temporary files may contain:

* private PDFs
* screenshots
* decrypted output
* media
* OCR text

Use private permissions.

Clean up after:

* completion
* cancellation
* crash recovery

At startup, safely detect abandoned Arcade Box temp directories and clean them according to policy.

Never recursively delete a path unless its ownership/scope has been verified.

---

# 47. SYMLINK AND PATH SAFETY

File operations must defend against:

* path traversal
* malicious archive paths
* unsafe symlinks
* accidental writes outside target directories
* archive zip-slip equivalents

Canonicalize and validate where required.

Archive extraction is particularly important.

---

# 48. CLIPBOARD PRIVACY

Clipboard history must be opt-in.

When enabled:

* allow expiry
* allow pinned items
* allow clear-all
* avoid writing full sensitive clipboard data into normal logs
* consider detection/suppression of password-like content
* offer application exclusions where platforms permit

Arcade Box must remain useful with clipboard history disabled.

---

# 49. NETWORK PERMISSIONS FOR PLUGINS

Plugins should declare network requirements.

Where practical allow declarations such as:

* no network
* arbitrary network
* specific domains

The UI must expose network permission clearly.

Changing a plugin from no-network to network access during an update counts as a permission escalation.

---

# 50. TELEMETRY

Do not make invasive telemetry part of an open-source privacy utility.

Default preference:

* telemetry off unless explicitly opted in

If telemetry is ever offered:

* publish exact event schema
* collect no file contents
* collect no clipboard contents
* avoid raw paths
* allow complete disablement

Crash reporting should be opt-in or transparently configurable.

Local diagnostics are encouraged.

---

# 51. PERFORMANCE TARGETS

Create repeatable benchmarks rather than claiming Arcade Box is “fast.”

Targets on representative modern hardware:

### Warm invocation

Global shortcut → useful first frame:

**target p95 below ~100 ms where platform behavior permits**

### Search

Query → ranked result update:

**target p95 below ~25 ms**

for the expected local tool catalog.

### Idle CPU

Effectively zero when nothing is happening.

No polling loops that continuously wake the CPU without reason.

### Background core memory

Target a small resident footprint.

Initial engineering target:

**roughly ≤35 MB RSS for the core/background layer where realistically measurable**

### Active lightweight UI

Avoid runaway WebView memory.

Establish per-platform budgets and continuously benchmark them.

A reasonable engineering target is approximately:

**≤120 MB total Arcade-owned active UI footprint for ordinary usage**, excluding clearly identifiable shared OS WebView infrastructure where measurement makes that distinction meaningful.

Do not cheat benchmarks.

### Large files

Processing a 20 GB video should not attempt to load it entirely into RAM.

Stream/process through providers.

---

# 52. STARTUP MODEL

Arcade Box should feel instantaneous.

Investigate the best design between:

* tiny resident core + lazily shown UI
* persistent lightweight island webview
* prewarmed UI
* platform-specific optimization

Do not keep the entire dashboard, FFmpeg, browser, OCR system, and plugin ecosystem active merely to make the shortcut fast.

Measure actual cold and warm behavior.

---

# 53. CANCELLATION

Every long-running operation must support cancellation when the backend permits it.

Cancellation must:

* stop worker process/task
* close file handles
* clean partial outputs where appropriate
* preserve explicitly requested partial results only when useful
* leave the runtime valid

Do not leave orphan FFmpeg/yt-dlp processes.

---

# 54. ERROR DESIGN

Errors must be actionable.

Bad:

> Error 1

Good:

> FFmpeg could not decode this stream.
>
> Codec: …
> Provider: `/usr/bin/ffmpeg`
>
> [Show technical details] [Try managed FFmpeg] [Copy diagnostics]

Technical logs belong behind expandable details.

Normal users should receive understandable messages.

---

# 55. PROVIDER HEALTH PAGE

Settings should contain:

**Engines & Dependencies**

Example:

FFmpeg
System
Compatible
`/usr/bin/ffmpeg`

yt-dlp
Arcade Managed
Current

Tesseract
Not installed
[Install]

LibreOffice
System
Detected

Users can choose between compatible providers where multiple exist.

---

# 56. UPDATE SYSTEM

Core application updates must be signed.

Use Tauri's supported signed updater architecture if Tauri remains the chosen framework.

Support GitHub Releases cleanly.

Updates must never execute unsigned downloaded code.

Plugin updates and managed-provider updates require their own integrity/signature verification.

Core app, plugins, and provider packages should be independently versionable.

---

# 57. RELEASE PACKAGING

Produce first-class releases.

Windows:

* signed installer when release infrastructure permits
* MSI/NSIS as appropriate
* x64
* ARM64 where supported

macOS:

* `.app`
* DMG or appropriate installer
* signing/notarization for production distribution
* Apple Silicon
* Intel/universal strategy based on build testing

Linux:

At minimum investigate:

* AppImage
* `.deb`
* `.rpm`
* Flatpak

Do not assume one artifact satisfies every Linux desktop.

Community AUR/Homebrew/Winget packages can come from documented packaging flows.

---

# 58. GITHUB RELEASE AUTOMATION

CI should build release candidates on actual target operating systems.

Use GitHub Actions or equivalent open infrastructure.

Produce:

* binaries
* checksums
* signatures
* changelog
* SBOM where practical
* third-party notices

Use reproducible builds wherever realistically achievable.

---

# 59. REPOSITORY ORGANIZATION

Design a monorepo layout intentionally.

A reasonable conceptual shape:

```text
arcade-box/

  apps/
    desktop/
    cli/

  crates/
    core/
    search/
    tool-runtime/
    pipeline/
    provider-broker/
    plugin-host/
    platform/
    jobs/
    storage/
    security/

  platform/
    windows/
    macos/
    linux/

  sdk/
    schema/
    wit/
    rust/
    typescript/

  tools/
    first-party/
      pdf/
      image/
      video/
      ...

  providers/
    ffmpeg/
    ytdlp/
    ocr/
    pdf/
    archive/

  docs/
    architecture/
    tools/
    sdk/
    platform/
    security/

  tests/
    fixtures/
    integration/
    e2e/

  scripts/
```

Do not copy this blindly if a better structure emerges.

Preserve the architectural separation it represents.

---

# 60. FIRST-PARTY TOOL REQUIREMENT

First-party tools must not bypass the public tool system simply because they live in the monorepo.

If a first-party tool needs something powerful, expose that operation through an appropriate host/provider capability that third-party tools could also request subject to permissions.

This dogfoods the SDK continuously.

---

# 61. PLUGIN SDK EXPERIENCE

Creating a basic tool should be pleasant.

Ideal developer flow:

```text
arcadebox sdk new my-tool
cd my-tool
arcadebox sdk dev
```

Developer mode should:

* hot reload manifest/UI where practical
* validate permissions
* validate schemas
* test inputs
* show logs
* run fixtures
* package plugin

Provide a complete example plugin that lives outside the first-party source tree and proves the public SDK is actually usable.

---

# 62. API VERSIONING

Tool/plugin API contracts must be versioned from day one.

Do not break every community plugin because a Rust struct changed.

Separate:

* Arcade Box application version
* Tool API version
* plugin version
* provider version

Have explicit compatibility rules.

---

# 63. PIPELINE VERSIONING

Saved pipelines must reference stable tool IDs.

When a plugin updates and changes I/O schema:

* detect incompatibility
* migrate when safe
* otherwise clearly identify broken stage

Never silently reinterpret old workflows.

---

# 64. SEARCH INDEX AS DATA

Tool definitions should automatically feed the search index.

Do not maintain a second hardcoded search list in the frontend.

Manifest metadata is authoritative.

Dashboard categories, search, plugin details, and documentation should all derive from the same tool registry.

---

# 65. ACCESSIBILITY

All primary workflows must be keyboard operable.

Requirements:

* logical tab order
* visible focus
* screen-reader labels
* reduced motion
* scalable text
* high-contrast compatibility
* no information conveyed only by color

The Island should be genuinely usable without a mouse.

---

# 66. INTERNATIONALIZATION

Architect for localization from the beginning.

Do not concatenate English sentence fragments in code.

English may be the first complete language, but strings should use an i18n system.

Tool manifests need localizable user-facing fields.

Search alias localization can be added progressively.

---

# 67. THEMING

Support:

* system
* light
* dark

Allow accent customization eventually without turning Arcade Box into a theming project.

Design must remain coherent.

Persist theme preferences.

---

# 68. ICONOGRAPHY

Choose a consistent open-source icon family suitable for distribution.

Do not randomly mix:

* Lucide
* Material
* Font Awesome
* custom SVG
* emojis

unless a deliberate design case exists.

Create custom icons only when existing ones fail conceptually.

---

# 69. TOOL FAVORITES

Users can favorite tools.

Favorites affect:

* dashboard
* search ranking

But exact query relevance always outranks generic favorites.

---

# 70. ALIASES

Users may create custom aliases.

Example:

`mp3`

→ Extract Audio from Video

Example:

`clean image`

→ saved pipeline

Alias handling must not modify global shell aliases.

It belongs to Arcade Box.

---

# 71. QUICK PARAMETERS IN SEARCH

Design search so certain tools can eventually accept lightweight arguments directly.

Examples:

```text
calc 549 * 87
```

```text
sha256 file.iso
```

```text
resize image.png 1920
```

```text
convert 14 GB MB
```

Do not require this for every tool initially.

Build parsing hooks cleanly enough to support it.

---

# 72. USER SAFETY FOR FILE OPERATIONS

Default pattern:

input:

`report.pdf`

output:

`report-compressed.pdf`

Do not overwrite `report.pdf` unless the user intentionally requests replacement.

For destructive operations:

* preview
* confirmation
* undo where technically practical

---

# 73. CRASH RECOVERY

Maintain sufficient job state to recover gracefully.

After unexpected crash:

* clean abandoned temp artifacts safely
* report interrupted jobs
* restore UI normally
* do not automatically resume dangerous operations without verification

Crash recovery must not become a source of accidental file deletion.

---

# 74. LARGE JOB ARCHITECTURE

Use async/nonblocking architecture.

The UI event loop must never wait on:

* compression
* network download
* hashing large files
* OCR
* FFmpeg
* scanning directories

Jobs emit structured progress.

Rate-limit UI progress events so large jobs do not produce thousands of frontend messages per second.

---

# 75. LOGGING

Use structured logs.

Levels:

* error
* warn
* info
* debug
* trace

Never log secrets.

Avoid raw clipboard contents.

Avoid document contents.

Redact sensitive query parameters/tokens.

Provide:

> Copy diagnostics

that gathers safe system/provider information.

---

# 76. HARDWARE ACCELERATION

Use hardware acceleration when it provides meaningful benefit.

Media providers may detect:

* NVENC
* AMD
* Intel Quick Sync
* VideoToolbox
* other current platform encoders

Do not choose hardware encoding blindly.

Expose:

* Auto
* Software
* available compatible hardware choices

Default Auto must favor quality/stability rather than merely using any detected GPU.

---

# 77. MODEL ACCELERATION

For optional ML image tools, abstract hardware execution.

Possible backends may include current:

* DirectML
* Core ML
* CUDA
* Vulkan
* ONNX Runtime execution providers

Do not hard-wire the application to one GPU vendor.

Validate current ecosystem state before choosing.

---

# 78. SECURITY THREAT MODEL

Write:

`docs/security/threat-model.md`

Cover at minimum:

* malicious plugin
* compromised registry
* malicious archive
* hostile filename
* hostile webpage
* malicious QR code
* malicious deep link
* PATH hijacking
* fake provider executable
* symlink attack
* partial download replacement
* update compromise
* credential leakage
* temp artifact leakage
* plugin permission escalation

Security design must be proactive rather than patched on later.

---

# 79. PROVIDER DISCOVERY SECURITY

The requirement to use globally installed dependencies does not mean executing the first arbitrary `ffmpeg` found blindly.

Provider discovery should:

1. resolve path
2. inspect executable
3. run a safe version probe
4. verify required capability
5. record provenance/path
6. warn if something is suspicious
7. allow user override

Never invoke discovered binaries through shell expansion.

---

# 80. TESTING STRATEGY

Testing must exist at several layers.

## Unit tests

For:

* parsers
* schemas
* ranking
* pipeline type checking
* provider discovery
* path safety
* manifest validation

## Integration tests

For:

* FFmpeg adapter
* yt-dlp adapter
* PDF providers
* OCR
* archive extraction
* plugin runtime
* provider fallback

## Golden fixtures

Maintain small legal test fixtures:

* PDFs
* images
* videos
* audio
* archives
* subtitles
* QR codes
* structured data

## End-to-end

Test real workflows:

hotkey
→ search
→ run tool
→ produce output

## Security tests

Include:

* archive traversal
* malicious filenames
* invalid manifests
* permission denial
* plugin network denial

## Fuzz testing

Use fuzzing for high-risk parsers/interfaces where useful.

---

# 81. CROSS-PLATFORM CI

CI matrix must cover actual platform compilation.

At suitable cadence run functional tests on:

* Windows
* macOS
* Linux

Do not allow Linux-only compilation to masquerade as cross-platform support.

Platform-specific code must be exercised.

---

# 82. VISUAL REGRESSION TESTING

Because UX is a core product feature, maintain screenshots/golden UI tests for important surfaces:

* compact Island
* search results
* tool expanded state
* dashboard
* permission prompt
* result state
* error state
* light/dark

Do not let parallel agents gradually destroy visual consistency.

---

# 83. PERFORMANCE REGRESSION TESTING

Create benchmarks for:

* search
* manifest loading
* tool discovery
* cold startup
* warm invocation
* SQLite startup
* plugin invocation
* pipeline scheduling
* large-directory scanning

Record baselines.

Regression should be visible in CI/reporting.

---

# 84. NO FAKE COMPLETION

A tool is not complete because:

* the card exists
* UI exists
* button invokes TODO
* mocked output appears

A tool is complete only when:

* real backend works
* errors are handled
* cancellation works where required
* test fixture succeeds
* result UI works
* docs exist
* search metadata exists
* permissions are correct
* platform limitations are documented

---

# 85. IMPLEMENTATION STRATEGY

Do not attempt to write 120 unrelated tools simultaneously before establishing shared infrastructure.

Build the reusable foundations first:

* repository
* core
* plugin/tool API
* tool manifest
* provider broker
* search
* job runtime
* pipeline engine
* platform abstraction
* Island
* dashboard
* permissions
* testing framework

Then implement tool families in waves based on shared backend infrastructure.

For example, once the FFmpeg provider is stable, many video/audio tools become thin, well-tested tool definitions over common media services rather than 17 independent implementations.

Likewise:

* PDF foundation powers PDF tools
* libvips foundation powers image tools
* OCR foundation powers several tools
* ZXing foundation powers QR/barcode tools
* archive provider powers archive utilities


---

# 86. DO NOT OVERENGINEER INDIVIDUAL TOOLS

Architecture should scale deeply.

Individual utilities should remain simple when the problem is simple.

A case converter does not need a microservice.

A checksum tool does not need AI.

A QR generator does not need a database.

Spend complexity where it multiplies across the product.

---

# 87. NO AI REQUIREMENT FOR CORE PRODUCT

Arcade Box must remain an excellent utility suite with no AI account.

Do not make users provide an API key just to search tools.

Optional AI-powered plugins/features may exist later.

They must follow LOCAL/CLOUD disclosure rules.

---

# 88. FIRST RUN EXPERIENCE

First launch should be short.

Suggested flow:

1. Welcome to Arcade Box.
2. Choose/test global shortcut.
3. Explain:

   * hotkey
   * type tool
   * Enter
   * Escape
4. Optional:

   * launch on startup
   * clipboard history
5. Finish.

Do not show a 14-page onboarding wizard.

After setup, open Arcade Island and encourage the user to type something.

---

# 89. PERMISSION EXPERIENCE

Permissions should be understandable.

Bad:

> `fs:scope-v2 allow-write`

Good:

> **QR Batch Generator wants to save files to the folder you selected.**

Technical details can be expandable.

---

# 90. PLATFORM FEATURE MATRIX

Maintain a generated/maintained matrix documenting support.

Example dimensions:

* global shortcut
* screenshot
* screen record
* system audio
* always on top
* app discovery
* clipboard image
* startup management
* file associations

Across:

* Windows
* macOS
* Wayland
* X11

Never hide known limitations.

---

# 91. DEPENDENCY FEATURE MATRIX

Maintain:

`docs/providers.md`

For every provider:

* purpose
* minimum tested version/capability
* system discovery
* managed availability
* license
* supported OS
* fallback

Do not hardcode assumptions that become invisible tribal knowledge.

---

# 92. TOOL MATRIX

Maintain a machine-generated or machine-verified catalog containing all 120 tools and later additions.

Fields:

* ID
* status
* category
* backend/provider
* LOCAL/NETWORK/CLOUD
* Windows
* macOS
* Wayland
* X11
* pipeline input
* pipeline output
* tests

This becomes the truth source for release readiness.

---

# 93. DESIGN DOCUMENTS REQUIRED

Before substantial implementation settles, maintain living docs for:

* product vision
* architecture
* tool API
* plugin model
* provider model
* pipeline model
* security threat model
* platform abstraction
* search design
* storage schema
* updater
* release process

Do not spend weeks writing theoretical documentation before coding.

Keep docs evolving beside implementation.

---

# 94. DECISION RECORDS

Use lightweight ADRs for irreversible or expensive choices.

Examples:

* plugin runtime
* application framework
* PDF backend
* updater architecture
* SQLite schema direction
* license
* Linux portal strategy

Record:

* problem
* options
* decision
* reason
* consequences

---

# 95. GIT DISCIPLINE

Use small meaningful commits.

Do not make one 500-file “implemented Arcade Box” commit.

Avoid leaving main broken for long periods.

Agents working in parallel must not overwrite one another's changes.


# 97. RESEARCH REQUIREMENT

Before selecting or integrating significant external components, verify the **current upstream state**.

Do not rely on stale model memory for:

* latest stable APIs
* Tauri
* yt-dlp
* FFmpeg
* OCR engines
* PDF libraries
* libvips
* ZXing
* WASI/Wasmtime
* platform APIs
* package licenses
* supported OS versions

Prefer:

1. official docs
2. official repositories
3. primary standards documentation

Record material findings.

---

# 98. DO NOT REIMPLEMENT MATURE INFRASTRUCTURE

Examples:

Do not write:

* a video codec
* PDF parser
* QR decoder
* ZIP engine
* OCR neural network
* YouTube extractor

merely because Arcade Box is written in Rust.

Use trusted mature software behind clean Arcade interfaces.

Our engineering value is in:

* integration
* consistency
* UX
* portability
* privacy
* discoverability
* automation
* safety

---

# 99. BUT DO NOT MAKE THE APP A SHELL-SCRIPT COLLECTION

External engines must sit behind typed APIs.

Bad architecture:

frontend
→ arbitrary shell command string

Good architecture:

Tool
→ typed capability
→ provider broker
→ validated provider implementation
→ structured result

This lets providers change without breaking tools.

---

# 100. RELEASE QUALITY BAR

Before calling the first complete public release ready:

* Arcade Island works reliably
* search is immediate
* dashboard is polished
* global shortcut works across supported environments
* plugin API is real
* sample external plugin works
* pipelines work
* system dependency reuse works
* managed-provider fallback works
* updates are signed
* permissions are enforced
* first-party tools are implemented rather than placeholders
* CI is green across supported OSes
* no critical security findings remain
* no known data-loss bugs remain
* docs are usable
* packaging is tested

Do not release simply because a date arrived.

---

# 101. CRITICAL ACCEPTANCE TEST: EXISTING FFMPEG

On a machine where compatible FFmpeg/ffprobe are globally installed:

1. install fresh Arcade Box
2. invoke Video Convert
3. Arcade Box discovers system FFmpeg
4. no duplicate FFmpeg download occurs
5. conversion succeeds
6. Engines settings show exact provider/path

Repeat analogous behavior for other supported global dependencies.

This requirement comes directly from product design and must be tested.

---

# 102. CRITICAL ACCEPTANCE TEST: ISLAND

From another application:

1. press Arcade shortcut
2. type `pdf merge`
3. Merge PDFs appears immediately
4. Enter
5. Island transforms into Merge PDFs
6. drag two PDFs
7. reorder
8. merge
9. output appears
10. reveal/save/send onward
11. Escape
12. previous app regains focus

This flow should feel dramatically faster than opening a browser and searching for a PDF website.

---

# 103. CRITICAL ACCEPTANCE TEST: CONTEXT

Copy valid JSON.

Open Arcade Box.

Without typing, relevant actions should appear.

Choose Format Structured Data.

Formatted JSON appears.

Copy it.

Escape.

No cloud request occurs.

---

# 104. CRITICAL ACCEPTANCE TEST: SCREEN QR

Invoke screen action.

Select QR code.

Arcade Box decodes it locally.

Display destination before opening it.

The user clicks Open.

The browser opens.

---

# 105. CRITICAL ACCEPTANCE TEST: PIPELINE

Create:

Screen Selection
→ OCR
→ Translate
→ Copy

Save:

`Translate Screen`

Invoke from search.

Select screen region.

Receive translated clipboard result.

The user should never need to manually open four separate tools.

---

# 106. CRITICAL ACCEPTANCE TEST: COMMUNITY PLUGIN

Build a trivial external plugin outside the main source tree using only public documentation/SDK.

Install it through local developer mode.

Arcade Box:

* validates manifest
* shows permissions
* indexes it in search
* renders its UI
* executes it in the appropriate sandbox/runtime
* supports typed input/output
* uninstalls it cleanly

If this requires internal APIs, the plugin architecture is not finished.

---

# 107. CRITICAL ACCEPTANCE TEST: MALICIOUS PLUGIN

Create a test plugin with no network permission.

Attempt outbound network access.

It must fail.

Create a plugin with no arbitrary filesystem permission.

Attempt reading an unrelated private file.

It must fail.

Security cannot be decorative manifest text.

---

# 108. CRITICAL ACCEPTANCE TEST: LARGE MEDIA

Process a genuinely large video.

Verify:

* UI remains responsive
* memory remains bounded
* progress works
* cancellation works
* temporary artifacts are cleaned
* no entire-file IPC copying occurs

---

# 109. CRITICAL ACCEPTANCE TEST: WAYLAND

On at least one mainstream Wayland desktop:

* global shortcut
* screenshot
* screen selection
* screen recording where portal support exists
* clipboard
* notification
* Island placement

must be tested for real.

Do not mark Linux/Wayland supported based solely on compilation.

---

# 110. QUALITY PRIORITIES

When forced to trade off, prioritize in this order:

1. correctness
2. user data safety
3. ease of use
4. responsiveness
5. cross-platform consistency
6. visual quality
7. extensibility
8. feature quantity

However, do not use this ordering as permission to make Arcade Box ugly.

Excellent UX is a first-order requirement.

---

# 111. PRODUCT FEEL

Arcade Box should feel like:

* a native desktop superpower
* instant
* confident
* calm
* predictable
* private by default
* technically deep without exposing complexity unnecessarily

It should not feel like:

* an admin dashboard
* developer demo
* browser website wrapped as an app
* giant settings panel
* package manager
* terminal GUI
* 200-card “online tools” website

---

# 112. THE MOST IMPORTANT PRODUCT TEST

Whenever designing a feature, ask:

> Does this reduce the time between the user thinking “I need to do X” and having X done?

If not, reconsider it.

Arcade Box wins by making tiny tasks disappear.

---

# 113. BEGINNING THE PROJECT

Start by inspecting the workspace/repository state.

If the repository has not yet been initialized:

* initialize it cleanly
* create foundational documentation
* establish formatting/lint/test infrastructure
* create the architectural skeleton

Before committing heavily to dependencies, perform the relevant current upstream/license investigation.

Produce and maintain a concrete implementation plan.

Then execute it.

Do not merely return a plan and stop unless explicitly instructed to plan only.

Use agents.

Review their work.

Integrate continuously.

Test continuously.

Do not defer architecture needed for the plugin/runtime/provider/pipeline model on the assumption that it can be added cheaply later.

At the same time, keep implementation vertical enough that Arcade Box becomes usable early.

An early milestone should already demonstrate:

**hotkey → Island → search → real tool → real output**

and subsequent work should expand the real system rather than replace a prototype architecture.

---

# 114. FINAL DIRECTIVE

Build **Arcade Box** as though this repository could become the open-source utility layer people install immediately after setting up a new computer.

A person should eventually be able to remove dozens of miscellaneous utilities and stop uploading ordinary files to unknown “free online tools” websites because Arcade Box handles those tasks locally and consistently.

The core advantage is not simply having 120 tools.

It is:

**one interface**

**one shortcut**

**one search system**

**one permission model**

**one plugin ecosystem**

**one pipeline system**

**one polished design language**

covering an expanding universe of tools.

Protect that idea throughout every architectural and implementation decision.
