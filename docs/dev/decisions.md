# Decisions

Arcade Box's architecture decisions. Each record says what was decided and
why; later records say when they replace an earlier one.

| Record | Decision |
|---|---|
| [ADR 0001](adr/0001-platform-and-core.md) | Rust core with Tauri 2 and a Svelte UI, one core for the desktop app and the CLI |
| [ADR 0002](adr/0002-versioned-tool-contract.md) | A versioned, serialised tool contract instead of Rust types |
| [ADR 0003](adr/0003-provider-broker.md) | Tools name provider capabilities; a broker finds and runs the engines |
| [ADR 0004](adr/0004-extension-sandbox.md) | No native plugins in the process; plugins run as sandboxed WebAssembly components |
| [ADR 0005](adr/0005-license-policy.md) | The main licence (GPL-3.0-or-later) and the dependency audit |

Decisions made during the 0.4.0 overhaul:

- **Folders and identifier.** Box keeps its files in Arcade Link's `arcade/box`
  folders and uses the OS identifier `dev.arcade.Box`. Earlier folders are
  copied, verified and then removed on first start (`arcade_core::dirs`).
- **Login entry name.** The login entry keeps the `arcade-box` name on Linux
  and `arcade.box.plist` on macOS, because Arcade Tools turns start at login on
  and off through the same file. Renaming it needs Tools to change first.
- **One version.** The workspace `Cargo.toml` holds Box's version; the desktop
  app, the CLI, the bundle and the frontend read it.
