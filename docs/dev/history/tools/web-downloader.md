# Universal Media Downloader (`arcade.web.downloader`)

**Input:** one public HTTP(S) media URL. **Output:** downloaded media and any requested subtitle, thumbnail, or metadata sidecars as app-owned artifacts or files in a user-selected folder. **Privacy:** NETWORK; the URL is sent to the source and yt-dlp uses its supported public extractors.

Select video or audio, then quality and output format. The tool reuses a compatible system yt-dlp and optional compatible Deno/Node runtime; it never reads browser cookies or yt-dlp configuration/plugins. Files are named after the media title (up to 90 bytes). No playlist expansion, remote component download, DRM bypass, or access-control bypass is allowed. Some sources expose fewer formats without a supported JavaScript runtime. A managed yt-dlp provider and full runtime setup remain future work.
