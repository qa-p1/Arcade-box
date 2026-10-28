# ADR 0001: Rust core with Tauri and a lightweight web UI

- **Status:** Accepted as the initial repository direction; release versions and capabilities remain subject to verification.
- **Problem:** Arcade Box needs a small native shell, shared GUI/CLI runtime, and strong file/process boundaries across desktop platforms.
- **Decision:** Use a Rust core, Tauri 2 desktop shell, TypeScript/Svelte UI, and SQLite for queryable local metadata. Keep the command facade narrow and share the core with CLI.
- **Reason:** This matches the latency, platform, and UI goals while keeping job, provider, storage, and permission logic outside the frontend.
- **Consequences:** Tauri capability configuration and platform adapters need review. Verify current supported OS versions, updater behavior, Wayland limitations, and dependency licenses before shipping. No broad shell or filesystem bridge is granted to the webview.
