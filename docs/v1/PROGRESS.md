# Arcade Box v1 progress

Detail for `qa-p1/Arcade-catalog` › `dev/v1/PROGRESS.md`. Newest first.

## 2026-11-14 (cloud session)

- **Step 5, memory (D14):** U1 unpacked runtime, W1 + W3b release after the last window, W0 after startup and after
  hidden Link work, OPEN1 open-window trimming (Box and its WebKit helpers via `libarcade-trim.so`), BX1 a 64 MiB
  budget for the clipboard history. Package and Tier 1 tests follow the resident instead of the launcher.

## 2026-11-13 (local session)

- **Step 3, the P2 fixes** (PR #35), each with a regression test that failed before:
  - `eb587db` B-11 text tools keep words and numbers whole; `76c0dec` + `a691b9a` B-14/BP-7 data types read as
    words; `d098b30` BP-8 Clear history asks first; `f27b67b` BR-14 a failed job always shows its reason.
  - `544da8a` B-24 image combine (same size) uses the median size and only shrinks the larger images.
  - `14b127d` B-20 passphrases from the bundled EFF list (7772 words); generated secrets rated by entropy.
  - `6a4fa6f` B-21 EAN/UPC: no zero padding, check digit checked, plain encoder errors.
  - `0bf9925` B-22 developer tools accept what people paste (Base64 variants, `0x` hex, `Bearer` JWTs, bare
    domains, cron names); MD5/SHA-1 as legacy checksums.
  - `8055050` B-23 unlock refuses an unencrypted PDF; no temp paths in errors; CLI, Link and pipelines get the
    form's visible defaults (`fill_ui_defaults`).
  - `604f766` B-17/18/19 colour, count and price input; `07f7ad4` B-13/14/15 toolbar, tooltip, validation text,
    portal error; `e094e7d` B-16 readability only for Latin text; `5afc750` B-3/B-4 sticky Island footer and a
    dimmed result once the input changes.
  - `553aeb9` B-26 X11 area capture uses the pre-overlay snapshot; `ef5a2c3` BR-13 colour picking uses Lens only
    to capture; `0eebcca` BP-2 DRM note only for access restrictions, BP-3 small images OCR'd at 2x.
  - `a14ca4d` one clipboard history: with Arcade Clipboard installed Box records nothing (Settings says so).
  - BP-4 (legacy `dev.arcadebox.app` icons) is fixed in Link 1.0.7 (the installer retires icons under an old
    desktop id); Box gets it when it vendors 1.0.7.
- **CI:** `3b63846` + `61d67df` apt installs with a deadline and retries (`scripts/apt-install.sh`).
- **Backlog (not v1, reasons):**
  - BP-1, PDF tools write beside the input rather than to a chosen folder: 14 publish points and settled
    behaviour; needs its own design (save dialog vs. output folder setting).
  - BP-5, the Island window is rebuilt on every show: fixing it is a window-architecture change (keep one hidden
    window alive), out of scope for v1's risk budget.
  - Watermarks with non-ASCII text (B-23): qpdf's stamp needs a bundled font with wide coverage; adds size and a
    licence entry.
  - BP-6 (zombie children) is Lens's LN-3, fixed there (Lens 34e97e3).

## 2026-11-11 (local session, owner's PC)

- **Step 3 completed in code** (PR #35; each item with its lock test flipped or a new check):
  - `64d5bc6` BR-3 (P0): the AppImage bundles GStreamer's playback plugins (D9); `verify-appimage.py` fails if one
    of 12 is missing. The native notices check will name the new Ubuntu packages on the first CI run.
  - `fae361d` BR-7: Document to PDF converts UTF-8 `.txt` and `.md` (headings, bold, nested lists, tables, quotes,
    code).
  - `4451856` + `9324a6a` BR-8: `providers` and the Engines page list every engine, found or missing (zbarimg is
    no Box engine: barcodes use the built-in zxing).
  - `67db520` BR-9: Window Pin and Paste plain act on the window the Island opened over.
  - `8ee70e3` CLI-2: the packaged-AppImage check covers an unknown argument (exit 2 with usage).
  - `65eee2e` BR-2 (P0): on Hyprland the Island's window follows its card, because Hyprland 0.56 hit-tests floating
    windows by their box and ignores the input region (D10). Tier 1 check flipped from known bug.
  - Open: H.264/AAC previews and BR-10 (recorder) under the harness (D9).

## 2026-11-11 (session 4)

- **Step 3, P1 fixes in the core.** Each commit removes its finding's `known_bug_` marker in
  `crates/arcade-core/tests/lock_findings.rs`; every one of those tests failed for the finding's reason before
  (session 2). Choices for ambiguous input are in D7.
  - `7c1a8a3` B-7 calculator: degree mode (new "Angles" control), `%` as percentage or remainder, `mod`, `!`, `**`,
    thousands separators, no float noise.
  - `949007d` B-8 units: decimal comma and exponents (`1,5 m`, `1.5e3 m`).
  - `2e580c4` B-9 date span: a negative span is negative in hours, minutes and seconds too.
  - `287b415` + `956d6b8` B-9 time zones: lower-case IANA names, common abbreviations (IST, PST, EST, …), `9:00`, `5pm`,
    `tomorrow 5pm` and Unix timestamps. Its lock test was committed first as a known bug and failed with every
    input rejected (or EST read as a fixed UTC-05:00 in July).
  - `ce44a75` B-10 diff: line endings and the final newline no longer make identical texts differ.
  - `febd7fe` B-25 process finder: threads are no longer listed as processes.
  - `a6c8fe1` U-1: a number or boolean given for a text or select option (CLI, Link) is used.
  - `bc8c093` BR-15 / BR-16 downloads: a failed HEAD no longer decides the length; a missing Content-Type is kept.
  - **Tested locally:** `cargo +1.88.0 test -p arcade-core` (all pass; BR-4 and BR-7 stay known bugs),
    `cargo clippy -p arcade-core --all-targets -D warnings`, `cargo fmt --check`,
    `python3 catalog/render_tool_matrix.py --check`, `node --test apps/desktop/frontend/tests/component-render.test.mjs`.
    CI not run yet.
- **Island and shortcut (P0/P1):**
  - `5668b87` BR-0 (P0): Escape in a tool returns to the search and a running job stays reachable (an indicator
    with Open Jobs). Escape clears a search with text and closes only the empty search. BR-5: the footer names what
    Escape does. BR-6: the footer and Settings say Ctrl+Enter runs a tool. BR-11: the highlight follows the
    pointer only when it really moves. BR-12: a hidden Island comes back as a fresh search with focus (D8).
  - `a869c08` BR-1: an Activated a second after the last one is a new press even with no release seen; key
    repeat still toggles once (unit tests `a_held_key_toggles_once`, `a_lost_release_does_not_wedge_the_shortcut`).
  - `a5e4c42` BR-4 (P0) search: typed expressions go to their converter, read by that converter's own parser
    (date, `#rrggbb`/`rgb()`, units with or without an amount, ISO 4217 currency pairs, arithmetic). Whole words
    rank before parts of words, and a tool's name before an alias that mentions the word. Background Remover
    gains the alias "remove background".
  - **Tested locally:** `node tests/island-keys.mjs` (all PASS, markers removed), `node tests/layout.mjs`,
    `npm test`, `svelte-check` (0 errors), `cargo test -p arcade-desktop --lib wayland_shortcut`,
    `cargo clippy -p arcade-desktop --all-targets -D warnings`, `cargo test -p arcade-core` (BR-4 lock flipped).
    **Not run locally:** the Tier 1 Hyprland test (`tests/desktop/island_desktop.py`, BR-0 and BR-1 checks
    flipped there too); CI runs it.
- **Open in step 3:** BR-2 (the Island's transparent area takes input; Box already sets a GTK input shape, so the
  cause must be found under the Hyprland harness), BR-3 (GStreamer plugins, packaging), BR-7 (text/Markdown to PDF),
  BR-8 (engines list), BR-9 (Window Pin), CLI-2, and the rest of B-8 the audit lists but the spec does not (time
  units, knot factor).

## 2026-11-10 (session 3)

- **Tier 1 "The shortcut opens the Island" (run 42578967289):** not a Box bug but a race in the test. After the
  restart, the test waited for the shortcut to be bound, but the portal stand-in still listed the first Box's
  session (a Box that quits never closes it), so the wait returned at once and the press went only to that dead
  session, before the new Box listened (box.log: the second `state=registered` came after the press). The test now
  waits for the restarted Box's own `state=registered` line. Box logs every step from `Activated` to the Island's
  WebView (stderr, box.log), and a failed check prints box.log and portal.log. Diagnosed with Fable's advice (owner
  rule). Harness follow-up for Link: the stand-in should drop a session when its owner leaves the bus.

## 2026-11-09 (session 2)

- **Step 0:** spec and audit copied into `docs/v1/` (D2); decisions log started (D1: stable waits for B1).
- **Step 1, behaviour lock:** known bugs pinned (D3) and today's intended behaviour locked:
  - Island keys (`island-keys.mjs`, in `scripts/check`): BR-0 Escape in a tool and the running-job link, BR-5 footer
    Escape hints, BR-6 run hints, BR-11 highlight under a resting pointer, BR-12 re-show; Escape clearing then
    closing, Enter opening the highlighted row and real pointer motion are locked as passing.
  - Core (`crates/arcade-core/tests/lock_findings.rs`): BR-4 search, B-7 calculator, B-8 units, B-9 negative spans,
    B-10 diff, B-25 threads as processes, U-1 numeric options, BR-7 text/Markdown to PDF, BR-15 failed HEAD, BR-16
    no Content-Type (both against a local HTTP server); passing locks for names ranking first, precedence, units,
    a real diff and a complete download.
  - CLI: `arcade-desktop` (CLI-1, CLI-3, CLI-4 pass; CLI-2 unknown argument known bug) and `arcade-box providers`
    (BR-8).
  - Tier 1 desktop (`tests/desktop/island_desktop.py`, new CI job on the built AppImage): identity, shortcut toggle,
    popup rule; BR-1, BR-2, BR-0 as known bugs.
