# Security policy

## Reporting

Report a vulnerability privately through GitHub's "Report a vulnerability" (Security tab of this repository), not in
a public issue. Expect an answer within a week. Fixes ship as a patch release through the normal rc → promote path. A
release found to be unsafe after it shipped gets the catalog's `bad` flag: Arcade Tools stops offering it and tells
installed copies to roll back.

## What Arcade Box guards, and how

| Surface | Protection | Tested by |
|---|---|---|
| Plugins | WebAssembly components run in a separate `arcade-plugin-worker` process, against the versioned Arcade tool interface only. WASI has an empty, deny-by-default context: no preopened folders, network or inherited handles. A plugin reads only the files the user passed it, through opaque handles, and only with the permission it declared and the user granted; a permission it gains in an update needs a new grant. Fuel, a wall-clock limit and a memory limit bound every run. | `crates/arcade-plugin-host/tests/plugin_security.rs` |
| Link IPC | Arcade Link's sockets (0600, peer UID check, per-endpoint token). | Link's own tests |
| Windows | Every window runs under a strict Content-Security-Policy with nothing remote: scripts only from Box's own files, connections only to Tauri's IPC. The Island and the dashboard allow inline styles (Svelte style attributes); the pinned-image window does not. The browser tests run every Island and dashboard surface under the policy and fail on any violation (BR-18). | `apps/desktop/frontend/csp.mjs`, `apps/desktop/frontend/pin.html` |
| Downloads and web tools | Box fetches only a URL the person typed or chose; nothing is fetched on its own. The URL must be HTTP or HTTPS with a host and no embedded credentials, and curl runs with `--proto =http,https --proto-redir =http,https` (at most 10 redirects), so a redirect cannot reach `file://` or another scheme. Local addresses are allowed, as in a browser: the request is the person's own. curl is started directly, never through a shell. A download is written to a private per-job staging folder and published only once it is complete (and matches the SHA-256 when one was given); the saved name comes from Content-Disposition or the URL and is reduced to a portable file name (no folders, `..` or reserved Windows names). Web-page-to-Markdown drops scripts, styles and `javascript:` links. | `crates/arcade-core/src/network.rs` (`rejects_unsafe_url_protocols_credentials_and_invalid_hosts`, `redirect_resolution_stays_on_explicit_http_protocols`), `crates/arcade-core/src/web.rs` (`content_disposition_and_url_filenames_are_sanitized`, `downloader_resume_requires_strong_etag_and_exact_range_metadata`, `expected_sha256_must_be_a_hex_digest`, `html_extractor_skips_active_content_and_rejects_script_urls`) |
| Media downloader | Uses the system's yt-dlp (Box does not ship or update it) with `--ignore-config --no-plugin-dirs --no-cache-dir --no-remote-components --no-playlist --restrict-filenames`: no user config, plugins or remote code from yt-dlp's side, one video per request, safe file names, output in the same private staging folder. A JavaScript runtime (Deno or Node) is passed only when one is installed. Box does not bypass DRM or paywalls. | `crates/arcade-core/src/web.rs`, `crates/arcade-core/src/web/ytdlp.rs` |
| Crash reports | Written locally under `arcade/box/crashes/`, never sent. Arcade Tools shows them, and the user may copy one or open a GitHub issue after reading it. | `apps/desktop/src-tauri/tests/cli.rs` |
| Updates | Box is installed and updated by Arcade Tools, which trusts a release only through its signed `manifest.json` (see Arcade Tools' SECURITY.md). | Arcade Tools' tests |

## Dependencies

`cargo deny check` (advisories, licences, bans, sources) runs in CI; Dependabot watches Cargo, npm and GitHub
Actions. Accepted advisories are listed with a reason in `deny.toml`: a timing issue in crypto-bigint 0.5, which
only age's P-256 recipients use (Box encrypts with X25519 and scrypt only), and "unmaintained" notices (no known
vulnerability) for crates that come through Tauri, gtk-rs, zbus 5, rav1e and age with no maintained replacement yet.
The bundled SQLite (3.53) can return stale memory when it opens a crafted database together with a `-wal` file of a
different page size (RUSTSEC-2026-0459). Box opens only its own `arcade.sqlite3` in its data folder, never a database
it was given, so only someone who can already write Box's data could plant such a pair. The fixed SQLite needs
rusqlite 0.41, which needs Rust 1.89; it lands when the Arcade fleet moves to that toolchain (`docs/v1/DECISIONS.md`
D6).
Plugins run on Wasmtime's 36 LTS line, which receives security fixes. Third-party licences, including the native
libraries in the Linux AppImage, are in `THIRD_PARTY_NOTICES.md` (`scripts/notices.py`, `scripts/native-notices.py`).
