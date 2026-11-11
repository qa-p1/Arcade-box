# Arcade Box v1 progress

Detail for `qa-p1/Arcade-catalog` › `dev/v1/PROGRESS.md`. Newest first.

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
