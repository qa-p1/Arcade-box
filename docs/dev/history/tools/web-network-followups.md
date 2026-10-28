# Web and network follow-up checklist

The catalog is the source of current status. These items remain before the web and network family is release-ready.

## Web

- [ ] **`arcade.web.downloader`** — Document a managed yt-dlp option and its update path; system yt-dlp is reused today. Authenticated downloads stay out of scope; never read browser cookies implicitly.
- [x] **`arcade.web.file-downloader`** — Strong-ETag `If-Range` resume, clean restart on mismatch, and optional SHA-256 verification before publication.
- [x] **`arcade.web.snapshot`** — Webpage to PDF or full-page PNG/JPEG with the user's browser: Chromium-family over the DevTools protocol (lazy-load scrolling), or Firefox-family headless screenshots (image-only PDF). Throwaway profile either way.
- [ ] **`arcade.web.markdown`** — Improve article extraction across real-world markup; the parser recognizes common static HTML only.
- [ ] **`arcade.web.images`** — Static HTML only; images inserted by JavaScript are not seen. A Chromium-rendered mode could reuse the snapshot session.
- [x] **`arcade.web.transcript`** — Exact-language captions with rolling auto-caption de-duplication; Groq Whisper fallback when a video has none.

## Network

- [x] **`arcade.network.site-check`** — "Is this site down?" with curl (status, timing, redirects, TLS) and the system `ping`; no third-party status API.
- [ ] **`arcade.network.rdap`** — Registry fallback for RDAP discovery errors and broader contact-data redaction fixtures; lookups use public `rdap.org`.
- [x] **`arcade.network.ip`** — Local interfaces through `sysinfo`, plus an optional public address from api.ipify.org.

## Release verification

- [ ] Run each tool on Windows and macOS, including missing-provider paths, cancellation, output publication, and network permission behavior, before changing their platform values from `conditional`.
