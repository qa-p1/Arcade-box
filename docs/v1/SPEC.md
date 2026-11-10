# Spec 3: Arcade Box 1.0.0

**Outcome:** Box 1.0.0. The Island behaves like a well-mannered popup:
- Escape goes back one step;
- the shortcut toggles it;
- its invisible area never captures the pointer;
- search finds what people type.

Also: every broken tool works or is hidden with a reason, media and screen recording work from the AppImage, Box is
packaged like the other Tauri apps (so its chunks are shared), and every fleet rule and gate passes.

Starting point: `qa-p1/Arcade-box`, Box 0.4.3. Read `AUDIT/box.md` (isolated, B-1..B-27 and U-1) and
`AUDIT/box-real.md` (real desktop: BR-0..BR-16 and BP-1..BP-8, including how the desktop was driven safely).

## Owner requirement (P0)

**BR-0:** Escape inside a tool returns to the Island's search. It never closes the Island. A running job keeps going
and stays reachable from the search (an indicator plus a link to Jobs). In the search, Escape clears the text first,
and closes only when the search is empty. Footer hints must match (B-2, BR-5).

## Findings to fix

| ID | P | What | Where |
|---|---|---|---|
| BR-0 / B-2 / BR-5 | P0 | Escape in a tool closes the whole Island. | Island key handling, `box.svelte.ts` |
| BR-3 / B-27 / BR-10 | P0 | No audio or video playback and no screen recorder. The AppImage forces `GST_PLUGIN_SYSTEM_PATH` to a bundled folder with zero plugins, though the host has `pipewiresrc`, `vp8enc` and `webmmux`. | packaging / AppRun env |
| BR-2 | P0 | The 740×740 Island's invisible area captures the pointer. The input region must match the visible card. | Tauri window / layer setup |
| BR-4 / B-6 | P0 | Search fails on the queries people type, for example `kg to lb`, `6*7`, `#ff8800`, `remove background`, `resize`. Typed expressions must route straight to Calculator, Units, Colour and Date. | search ranking |
| BR-11 / B-5 | P1 | The result highlight follows the stale pointer position, so Enter opens the wrong row. Keyboard selection wins until the pointer actually moves. | Island list |
| BR-12 | P1 | The old query and old tool survive a re-show, and the first keystrokes are lost. A re-show opens a fresh search with input focus ready before the first frame. | Island show path |
| BR-1 | P1 | The shortcut never closes the Island when only Activated arrives (synthetic dispatch). Reset `shortcut_down` on hide, or after 1 s, so a lost release can't wedge it. | `wayland_shortcut.rs:398-401` |
| BR-6 / B-1 | P1 | The Enter vs Ctrl+Enter hints contradict each other. | `IslandFooter.svelte:14`, `SettingsPage.svelte:24`, `box.svelte.ts:779` |
| BR-9 | P1 | Window Pin always fails ("No other window is active" next to "Done"). Capture the target window's address before hiding the Island. | `apps/desktop/src-tauri/src/hyprland.rs:168-190` |
| BR-15 | P1 | The downloader trusts a failed HEAD's length and discards a complete file. GET headers are authoritative; add `--fail` to the HEAD probe. | `crates/arcade-core/src/web.rs:196-199,431-470` |
| BR-16 | P1 | A download with no Content-Type is rejected (the trim eats the empty last field). | `crates/arcade-core/src/web.rs:518-522` |
| B-10 | P1 | Diff reports differences between identical texts; CRLF input is garbled. | `crates/arcade-core/src/text_tools.rs:447-455` |
| B-25 | P1 | Process Inspector lists threads as processes. | `arcade-core/src/system_tools*` |
| B-7 / B-8 / B-9 | P1 | Calculator float noise, no degree mode, unsupported common syntax (`%`, `!`, `mod`, thousands separators); Units decimal comma and exponent; DateTime negative-span signs; Time Zone input formats and abbreviations. | `converter_tools/units.rs`, `converter_tools/datetime.rs`, calculator |
| BR-7 | P1 | Document to PDF rejects `.txt` and `.md`. | converter input types |
| BR-8 / B-12 | P1 | The engines list misses tesseract, yt-dlp and zbarimg. "Implemented" tools fail when an optional engine is missing; say so up front. | providers / Engines page |
| U-1 | P1 | Numeric option values for string options are silently ignored (CLI and Link). | core option parsing |
| B-11 | P2 | Text tools: title case, final sigma, contractions, big-int JSON→float, duplicate keys, the invalid-JSON message, CSV rename. | text tools |
| B-20 / B-21 / B-22 / B-23 / B-24 | P2 | Password strength advice and passphrase words; barcode zero-padding and ZXing internals in errors; regex flags, hash list and decoders; PDF messages leaking temp paths, non-ASCII watermark; image "same size" shrinking to the smallest. | the tool modules named in `AUDIT/box.md` |
| B-3 / B-4 / B-13 / B-14 / B-15 / B-16 / B-17 / B-18 / B-19 | P2 | Footer clipped; stale result after input change; button wrap; raw MIME and clipped validation text; raw ZBus error; non-English readability; colour, percentage and lorem input handling. | see `AUDIT/box.md` |
| B-26 | P2 | X11 area screenshot includes the overlay tint. | screenshot capture |
| BR-13 | P2 | Pick screen colour doesn't return its result to the Island (OCR and QR do). | Lens handoff |
| BR-14 | P2 | Jobs page shows failures without a reason. | Jobs view |
| BP-1..BP-8 | P2 | Inconsistent output location; DRM text on a plain 404; OCR accuracy on small text; two icon identities; Island window recreated on every show; Lens zombie (fixed in Lens); raw MIME names in pipeline input types (`PipelinesView.svelte:544`); clearing history from the Island has no confirmation (`ClipboardHistoryView.svelte:115,156`). | see `AUDIT/box-real.md` |
| — | P2 | Two clipboard histories (Box's own and Clipboard's). When Clipboard is present, Box uses it and hides its own store. | Settings › Clipboard |

## Packaging (P1, size and memory)

Box's AppImage shares about 0% of its chunks with the other Tauri apps (`RESEARCH.md`: 118.5 of 119.7 MB needed
even with Tools or Look installed). Package it the same way Tools and Look are, with the same WebKitGTK bundle
layout, so the shared chunk store works. Bundle a correct GStreamer plugin set (`appsink`, `playbin`, `pipewiresrc`,
`vp8enc`, `webmmux`), or point at the host plugins when they exist. Never point at an empty folder. A CI check plays a
short audio and video clip and records 2 s of screen under the harness.

## Memory

Apply `AUDIT/memory.md` items U1, W1, OPEN1, W3b and BX1 (bound the 500-item history and job results). Never turn off compositing: the Island must stay smooth. Budget: background 60 PSS / 80 RSS.
Apply the Box items from `AUDIT/memory.md`: release the webview and network process on hide, the startup trim, and
running unpacked. Box's budget comes from `AUDIT/memory.md`.

## CI

Measured (`AUDIT/FINDINGS.md` CI2): main CI takes 47–61 minutes, and the Windows job alone about 51 minutes, dominated by compilation. Apply `PLAN.md` step 6:
- sccache across jobs;
- build the Link CLI once;
- Windows and macOS only on release and nightly, not on every PR (a PR runs a `cargo check` for those targets instead);
- shard the tool tests.

## Acceptance

All `PLAN.md` exit gates, plus:
- A harness test for every P0/P1 row.
- Every tool in the catalog runs once with a real input in CI. Tools that need optional engines are skipped with a
  reason when the engine is absent.
- Media and recorder smoke tests pass.
- Automated drag and drop: files in to a tool's input and results out, under the harness.
- Delta update from 0.4.3 to the rc downloads less than 30% of the file.
