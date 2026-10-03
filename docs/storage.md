# Storage and retention

Use SQLite for relational application metadata and migrations from the beginning. Do not place source documents, media, clipboard payloads, or large pipeline artifacts in database blobs. Use private files and opaque scoped references for job data. Migrations are versioned, transactional, and tested against both a fresh database and the previous supported schema.

## Initial logical tables

| Table | Purpose |
|---|---|
| `schema_migrations` | Applied migration identifiers and checksums. |
| `settings` | Typed application settings, theme, shortcut, and retention choices; never raw credentials. |
| `tool_state` | Installed/disabled tool metadata and manifest versions. |
| `aliases` | User-defined aliases tied to stable tool or pipeline IDs. |
| `favorites` | Favorite tool and pipeline IDs. |
| `usage_events` | Minimal local ranking signals; configurable retention and path-free by default. |
| `jobs` | Status, tool/pipeline reference, timing, safe output references, progress summary, and interruption state. |
| `pipelines` | Versioned serialized DAG definitions and friendly names. |
| `providers` | Discovery result, provenance, version, capabilities, and health timestamp. |
| `plugins` | Package identity, version, hash/signature, effective/requested permissions, enabled state. |
| `clipboard_items` | Created only when clipboard history is opted into; expiry, pin, exclusions, and sensitive-content policy apply. |

Secrets use the OS credential store (Windows Credential Manager, macOS Keychain, Linux Secret Service when available). If secure storage is unavailable, require an explicit safer fallback choice; never silently store API keys in SQLite. Logs and history omit document contents, secrets, raw clipboard content, and source paths when configured. Sensitive tools use minimal history by default.

On startup, abandoned temporary jobs are considered for cleanup only after ownership, scope, and expected directory identity have been verified. Crash recovery reports interrupted jobs without resuming dangerous operations automatically. Users can configure history retention and clear it explicitly.
