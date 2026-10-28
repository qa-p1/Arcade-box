# Direct File Downloader (`arcade.web.file-downloader`)

**Input:** one HTTP(S) URL. **Output:** one typed artifact with server filename/MIME where available. **Privacy:** NETWORK.

The tool uses the system curl provider, follows at most 10 HTTP(S) redirects, streams to private per-job staging, reports transfer progress, and publishes without overwriting. Downloads are capped at 200 GiB. It makes at most three transfer attempts. A failed transfer can resume only when the origin supplied a strong ETag and a known representation length; the retry sends `If-Range` and the tool verifies the final `206` status, matching ETag, exact `Content-Range` start, and total length. If any range check fails, staged bytes are discarded and a clean attempt starts. Partial files remain private and are never published.

The optional **Expected SHA-256** accepts exactly 64 hexadecimal characters. Arcade Box hashes the completed staged file and publishes it only when the digest matches. A mismatch reports the actual digest and discards the staged file. Checksum work is cancellation-aware. The destination is Arcade Box artifacts unless a user-selected folder is chosen. Cross-platform provider and transfer-fixture verification is still required before this tool can be marked complete.
