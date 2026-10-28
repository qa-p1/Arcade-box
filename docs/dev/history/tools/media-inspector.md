# Video / Media Inspector

**Status:** implemented. **Privacy:** LOCAL. **Input:** one user-selected audio or video file. **Output:** `structured/media-info` JSON with container, streams, chapters, and metadata.

Arcade Box discovers an existing FFmpeg/ffprobe pair, verifies executable identity and baseline probing capability, and runs ffprobe with machine-readable JSON output. The Island shows a concise summary and retains the technical JSON for inspection. The selected source is never changed.

If FFmpeg/ffprobe is unavailable or incompatible, Engines & Dependencies shows the path/version or the reason it cannot be used. Some format fields are absent when the container does not provide them. System provider behavior is tested on Linux; Windows/macOS and desktop-specific paths remain conditional until exercised in CI and on real machines.
