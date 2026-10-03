# Recovery audit — 27 September 2026

The original specification is the product source of truth. Catalog status describes implementation scope, not proof that a complete desktop flow works. This audit records evidence as recovery progresses.

## Baseline

- Existing Rust core, Tauri desktop, Svelte UI, CLI, SQLite, provider adapters, jobs, typed pipelines, and sandboxed plugin worker are retained.
- 120 catalog entries: 27 implemented, 92 partial, 1 planned. The checked-in tool matrix was stale and needs regeneration.
- No Git commits existed at audit start; all project files were untracked. A local source baseline was archived outside the repository before changes.
- Frontend production build and Svelte checks passed. Rust baseline: 4 contract, 100 core, 1 plugin protocol, 10 plugin integration tests passed. Another 27 tests were duplicated by a temporary module-import harness.
- Native embedded desktop launched on this Linux/Hyprland session and displayed onboarding. Platform-specific acceptance still needs interaction testing; compilation is not sufficient.

## Recovery order

1. Repair launch, dismissal/recovery, native copy, clipboard context, input routing, result consumption, and asynchronous navigation.
2. Verify an MVP of search, text/data tools, generators, selected-file tools, real results, favorites/history, and background work. Publish exact launch and test steps as soon as this checkpoint is usable.
3. Finish existing pipelines, provider health, drag-and-drop, previews, and remaining partial tool integrations in bounded slices.
4. Address important specification gaps, with honest platform/provider limits; polish and perform a final spec audit.

## Flow findings

| Area | Existing implementation | Verified defect / remaining work |
| --- | --- | --- |
| Launch | Native desktop and browser frontend | Plain cargo run used the Vite URL and could open another project on port 5173; embedded asset feature needed. |
| Invocation | Native or portal shortcut | No tray/reopen path when registration fails; hidden Island could become unreachable. |
| Startup | Catalog plus capability discovery | Onboarding and catalog waited for optional sequential engine probes. |
| Clipboard | Invocation-time local context | URL offered QR generation but command rejected QR; copy relied on browser clipboard instead of native plugin. |
| Input forms | Manifest-generated controls | Mixed text/file manifests and file/any[] routing need correction and regression tests. |
| Results | Typed text and file handles | Mixed file/text results hid reports; continuing a file lost display metadata; CLI JSON/pipelines leaked ephemeral handles. |
| Desktop actions | App opener and window pin | Both referenced obsolete window label main instead of island. |
| Jobs | Bounded workers, cancellation, events | Async results need navigation isolation; long tools should consistently use jobs. |
| Pipelines | Saved typed DAG runtime and linear editor | Missing input/output validation, editor/backend constraints, cancellation and resumable editing need audit. |
| UI | Island and full dashboard, themes | Tiny 7–10 px text, inconsistent result views and abrupt state changes need repair. |
| Catalog/docs | Broad implementations, per-tool notes | Readiness matrix and architecture notes lag behind source; partial features must remain honestly labeled. |
| Release/platform | CI and source platform adapters | Cross-OS runtime verification, signed release infrastructure and managed distribution are not established by local checks. |

## Video category completion (2026-10-02)

All ten Video tools are now implemented on one shared FFmpeg planner (`crates/arcade-core/src/media/video.rs`).
- **Trim:** stream-copies on keyframes.
- **Join:** concat-copies matching clips.
- **Compress:** does real two-pass target sizing.
- **Subtitles:** supports text and image-based tracks.
- **Convert, Crop, Compress, Extract Audio:** batch-process up to 50 files.

The Island adds visual editors (filmstrip range/point timeline, a crop area drawn on a real frame, a sampled size estimate, and audio/subtitle track pickers) through the `video_preview` and `estimate_video_output` commands. Codec and container choices the installed FFmpeg cannot provide are disabled through choice `requires`.

Remaining: the image-based subtitle burn path (PGS/VobSub `overlay`) has no fixture test, and the per-OS hardware encoders (NVENC/VAAPI/VideoToolbox) are not offered yet.

## Audio category completion (2026-10-02)

All seven Audio tools now run on one audio module (`crates/arcade-core/src/media/audio.rs`), which shares the job, batch, and publishing helpers with video (`media/job.rs`). Fixed defects from the earlier partial versions:
- Trim had a fixed 5 s end and ignored bitrate.
- Loudnorm output came out at 192 kHz.
- Speed/pitch required `rubberband` but never used it.
- Silence trim depended on version-specific `silenceremove` semantics.
- `.opus` files were rewritten as `.ogg` on tag edits.

New features:
- **Normalize:** real two-pass EBU R128 and a peak mode.
- **Trim:** lossless copy and cut-out-with-crossfade.
- **Join:** lossless joins, crossfades, and gaps.
- **Silence:** a pause-shortening mode.
- **Tags:** tag prefill, cover replace/remove, and "only listed tags".
- **Batch:** convert and normalize.

The Island adds a waveform range editor, a live silence map, a loudness meter, and a tag card.

Remaining: the non-rubberband pitch fallback isn't exercised on this machine (rubberband is installed), and WAV INFO tags cover only the common fields.

## Safety

No persistent shell, login, desktop startup, or system environment configuration was changed. Test data and environment overrides are scoped to test processes. Original files are preserved by output publication contracts.
