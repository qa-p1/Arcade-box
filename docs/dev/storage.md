# Storage and retention

Current implementation, checked against `crates/arcade-core/src/storage.rs`
and the desktop clipboard-history service on 2026-10-08.

## SQLite metadata

`arcade.sqlite3` stores application metadata. SQLite uses WAL, foreign keys
and numbered migrations, each applied in a transaction. Migration records
contain a version number, not a checksum.

| Table | Stored information |
|---|---|
| `schema_migrations` | Applied version numbers |
| `settings` | String key/value settings, including provider caches and pipeline approvals |
| `favorites` | Tool IDs |
| `usage` | Count and last-use time per tool |
| `history` | Tool ID, success/error status and timestamp |
| `custom_aliases` | Alias to tool ID |
| `pipelines` | ID, version, serialized definition and update time |
| `jobs` | ID, tool, status, progress and timestamps |
| `provider_choices` | Capability and executable path |

History and job records omit input content, source paths, output values and
error text. Results stay in memory; `arcade.security.*` tools skip usage and
history recording. File artifacts use scoped references rather than database
blobs. Saved pipeline options are persisted, but password controls are rejected.

The CLI and desktop share one database in Box's data folder,
`arcade_core::dirs::data_dir()` (Link's data folder for Box: `arcade/box` on Linux, `Arcade/Box` on Windows and macOS; under
`$ARCADE_HOME` when that is set). Before 0.4.0 they used separate folders
(`dev.arcadebox.app` and the CLI's `arcadebox` / `Arcade Box` folder);
`arcade_core::dirs::prepare()` moves both with Link's verified migration on
the first start, refuses while an old desktop still holds `desktop.lock`, and
leaves a conflicting older file in place and says so in the log. The
webview's own data lives in `webview` inside Box's cache folder.

## Files and credentials

Desktop clipboard history is opt-in and stored separately in
`clipboard-history.json` beside the desktop database. Its retention choices
are 1, 7, 30 or 90 days (default 7); pinned entries and exclusions follow the
clipboard-history service's rules. Plugin packages and their install metadata
are also files, not a `plugins` SQLite table.

Groq credentials are read on demand from `GROQ_API_KEY`, then the application
data directory's `.env`, then the workspace `.env` in debug builds only. Box
does not currently implement an OS credential-store adapter for these keys.
The `.env` fallback is plaintext; keep it private and out of version control.
Keys are not returned in results, logged, or put in provider argument vectors.

Interrupted jobs are marked during recovery and terminal job metadata can be
pruned. A general configurable retention policy for all tool history, migration
checksums and automatic credential-store migration are design work, not shipped
features. See [status](../STATUS.md) and [threat model](threat-model.md).
