# Arcade Box v1 decisions

Decisions the plan left open, made by the v1 session. Each one cites the part of catalog `dev/v1/README.md` or
`PLAN.md` it follows. Newest last.

## D1 — Stable promotion waits for the signing secret (blocker B1)

The `ARCADE_UPDATE_SIGNING_KEY` Actions secret does not exist and this session cannot create it (catalog
`dev/v1/PROGRESS.md` B1, Link and Tools D1). rc releases on the hidden `test` channel need no signature, so all rc work
and every rc exit gate continue; `promote.yml` refuses to run without the secret. README: "never publish unsigned
stable".

## D2 — Spec and audit copied at catalog `a33fc70`

`docs/v1/SPEC.md` is `specs/BOX.md`. `docs/v1/audit/` holds `box.md` with its text evidence (`box/`), `box-real.md`,
`FINDINGS.md`, `memory.md` (with Box's measurement files in `memory-box/`) and `conformance-baseline.md`, all from
catalog commit `a33fc70`. Screenshots stay in the catalog (PLAN: text only). Line references in the spec are for Box
0.4.3 (`497c160`).

## D3 — Known-bug tests (behaviour lock)

As in Tools (Tools D3): a Rust test for a finding that is not fixed yet is `known_bug_*` with
`#[should_panic(expected = "KNOWN BUG <id>")]`, its assertion naming the finding and what it saw. The Island's key
tests (`apps/desktop/frontend/tests/island-keys.mjs`, Playwright on the visual fixture) and the Tier 1 desktop test
(`tests/desktop/island_desktop.py`, Link's headless Hyprland harness) name such checks "KNOWN BUG <id>: …" and report
XFAIL; a fix makes them XPASS, which fails the run until the marker is removed with the fix. Each known-bug test was
checked to fail for the audit's reason (its message quotes what it saw), not by accident.

The desktop test runs in one harness job with several checks rather than one job per finding with Link's
`expect-exit: 10`: one Hyprland session instead of five, and a fixed finding shows up by name.

BR-12 (a re-show keeps the old tool) is locked in the key tests only: the harness cannot read the page, and the
re-show path is the same frontend code. BR-3/BR-10 (GStreamer) are packaging findings locked by the media smoke test
the spec asks for, which comes with the packaging change (the current AppImage has no plugins to test against).

## D4 — Two findings from the 1.0 work, fixed in step 3

BR-17 (no shortcut on xdg-desktop-portal < 1.19) and BR-18 (no CSP in the Island and dashboard) were found after the
audit, while moving Box to Link 1.0.6 (`docs/v1/audit/box-real.md`). They are P1. Step 3 left them open, so
they are fixed in step 7, each with a test that fails first:
- BR-17: a portal with no host Registry (an unknown method, interface or object, or no such portal interface) is logged
  and the shortcut is bound anyway; any other Register failure is still an error (`wayland_shortcut.rs` tests).
- BR-18: one policy (`frontend/csp.mjs`) is injected by a Vite plugin into the built `index.html` and
  `dashboard.html`. Nothing is remote: `default-src 'none'`, scripts from `'self'` only, images, media and fonts from
  `'self'`, `data:` and `blob:`, connections to `'self'`, Tauri's IPC and `data:`/`blob:`. Inline styles stay allowed
  because Svelte style attributes position the progress bars, the ruler and the colour sampler. `tests/csp.test.mjs`
  checks the plugin and the policy, and the layout and key tests serve the visual fixture under the same policy and
  fail on any violation Chromium logs (checked by dropping `'unsafe-inline'`: every key scenario then fails).
  Tauri's own `app.security.csp` stays unset: the policy lives in the pages, as `pin.html`'s always has.

## D5 — Dependencies: Wasmtime 36 LTS, cargo deny over the workspace

`cargo deny --workspace` found 30 advisories in Wasmtime 38.0.4, among them WASI permission bypasses and sandbox
escapes, plus zip 8 (two copies, one through rust_xlsxwriter), calamine 0.36, tokio and pulldown-cmark. Wasmtime 38 is
out of support. Its current line (50.0.1) needs Rust 1.97, and Box builds on 1.88 like the rest of the fleet, while
the 36 LTS line (36.0.20, Rust 1.86) has every fix and none of the later regressions (those advisories mark 36 as
unaffected). Box therefore pins Wasmtime and wasmtime-wasi to `=36.0.20`. All ten plugin sandbox tests pass on it
(`plugin_security.rs`). zip 9.5, calamine 0.38 and rust_xlsxwriter 0.102 clear the rest; `.cargo/config.toml` makes
Cargo pick versions that support Rust 1.88. The accepted advisories in `deny.toml` each carry a reason: the
crypto-bigint timing issue is in age's P-256 code, which Box never calls, and the rest are "unmaintained" notices
with no replacement available. Box's own crates are `publish = false` and GPL-3.0-or-later; `licenses.private`
keeps cargo deny from judging them.

## D6 — RUSTSEC-2026-0459 (bundled SQLite) is accepted until the fleet's Rust 1.89

Box PR #20's cargo deny found RUSTSEC-2026-0459, published after the D5 pass: SQLite before 3.54 can return
uninitialized page-cache memory when it opens a database with a `-wal` file of a different page size, so a crafted
pair can leak process memory to a query. `libsqlite3-sys` 0.38.2 (through rusqlite 0.40.2) bundles 3.53.2. The fix
is `libsqlite3-sys` 0.39, which rusqlite 0.40 does not accept (it requires ^0.38.2), and rusqlite 0.41 needs Rust
1.89, while Box builds on 1.88 like the rest of the fleet (D5). The advisory exposes only a process that opens a
database a less trusted party could have written. Box opens exactly one database, `arcade.sqlite3` in its own data
folder (`Arcade::open`, from the desktop app, the CLI and the Link handler), never a file it is given; whoever can
write that folder can already change Box's data and plugins. So it is accepted in `deny.toml` with that reason and
explained in SECURITY.md, and the upgrade to rusqlite 0.41 goes in with the fleet's move to Rust 1.89 (a fleet-wide
decision, not Box's alone). Clipboard also bundles SQLite (libsqlite3-sys 0.35) and is checked against this
advisory in its own step.

## D7 — How the converters read ambiguous input (B-7, B-8, B-9)

The fixes for the converter findings had to choose a reading where an input has two:

- **Calculator `%`.** `10 % 3` (an operand follows) is the remainder, like `10 mod 3`; `50%` with nothing after it
  is a percentage, and `100 + 10%` adds ten percent *of the left side* (110), as on a desk calculator, which is what
  people who type it mean. `!` is a factorial unless it starts `!=`. Angles are radians unless the new "Angles"
  control says degrees (the spec asks for a degree mode; it is a select on the existing tool, not a new tool).
  Results are rounded to 15 significant digits, so `sin(pi)` is 0 and `0.1 + 0.2` is 0.3.
- **Thousands separators and decimal commas.** A comma followed by exactly three digits after a one-to-three digit
  integer part is a thousands separator (`1,000`, `12,345,678`); any other single comma in a number with no point is a
  decimal comma (`1,5 m` is 1.5 m). Inside a calculator function's arguments a comma always separates arguments.
  `1,500` therefore reads as 1500, the English reading, because that is what Box's UI language shows.
- **Time zone abbreviations (B-9).** `EST`, `PST`, `IST` and the other common abbreviations each stand for their
  region's zone (America/New_York, America/Los_Angeles, Asia/Kolkata), so a date in July typed with `EST` gets New
  York's summer offset: people type "EST" to mean Eastern time, and a fixed UTC-05:00 in summer would give a meeting
  time an hour off. The label shows both (`America/New_York (EST)`), so the reading is visible. IST is India's, the
  most common use; Israel and Ireland are reachable by their IANA names. IANA names are matched in any case.
- **Times without a date (B-9)** (`9:00`, `5pm`) are today in the source zone; `tomorrow 5pm`, `today at 14:00` and a
  date followed by a time work too. Plain digits are Unix time with the unit picked by the digit count, the same rule
  the Date tool already used, now shared by every date input.

## D8 — The Island comes back fresh; footer checks allow chip and label without a space

- **BR-12 overrides the old design.** The Island kept its tool, input and result across a hide on purpose
  ("Keep tool input/results intact when the user temporarily puts us away"). The spec says a re-show opens a fresh
  search with focus ready, and the owner's spec wins. So the Island resets to the search when it is hidden
  (`island-hidden`, not on show, so an "open this tool" request that comes with a show is kept). A job started from
  the old tool keeps running and is reachable from the search's Jobs indicator (BR-0). The focus memory that
  restored the last focused element is removed with it.
- **The footer checks (`island-keys.mjs`) accept `EscClose` as well as `Esc Close`.** A key chip and its label are
  separate flex items, so `innerText` has no space between them and the check as written could never pass. The BR-6
  check beside it already allowed this (`(Ctrl|⌘) ?↵ Run`). Each check still requires the right label for its state
  (Close, Clear, Back), and the BR-5 check failed on the old code for the finding's reason: a search with text said
  Close.


## D9 — The AppImage bundles GStreamer's playback plugins (BR-3)

- **Problem:** WebKit plays audio and video through GStreamer. The AppImage points GStreamer at its own plugin folder,
  which held no plugins, so no media played (`appsink not found`, B-27).
- **Options:** stop overriding the plugin path and use the host's plugins (they link against the host's newer
  libgstreamer, not the bundled one, and fail or skip at random by distro), or bundle the plugins.
- **Decision:** bundle them with Tauri's `bundleMediaFramework`, from Ubuntu 24.04's `gstreamer1.0-plugins-base`,
  `-good` and `gstreamer1.0-pipewire` (LGPL). That covers WebM/VP8/VP9, Ogg/Vorbis/Opus, MP3, WAV, FLAC, Matroska and
  the MP4 container, with PulseAudio/PipeWire output. `gstreamer1.0-libav` (H.264/AAC) is not bundled: Ubuntu builds
  it against a GPL FFmpeg. H.264/AAC media does not preview from the image (open item: tell the user so in the
  preview). The screen recorder runs the host's GStreamer, clearing the image's plugin paths (BR-10: still to verify
  under the harness). Cost: the image grows by about 15–25 MB.
- **Test:** `scripts/verify-appimage.py` (CI and Release) fails if the image lacks any of the 12 plugins playback
  needs.

## D10 — On Hyprland the Island's window follows its card (BR-2)

- **Cause:** Box already limits the Island's input region to the card (`input_shape_combine_region`), and GTK
  3.24.41 applies it. But Hyprland 0.56 hit-tests a floating window by its window box and never consults the
  surface's input region (`src/desktop/state/ViewHitTester.cpp`, `CViewHitTester::windowAt`). So a click anywhere in
  the 740×740 canvas went to the Island.
- **Decision:** on Hyprland only, the native window follows the card: it grows at once and shrinks 360 ms after
  the card's target height drops, so a shrinking card is never clipped mid-animation. Each show restores the full
  canvas and fits again. The card's maximum height comes from the canvas the window had when shown
  (`--island-canvas`), not from the current viewport, so a shrunken window can't cap the card's growth. Other
  desktops honour the input region and keep the full canvas.
- **Test:** Tier 1 Hyprland checks the window is shorter than 400 px over the search card, and that a click where
  the old canvas was reaches the window underneath. Both were the BR-2 known-bug check.

## D11 — Dead code (step 4): how it is found and what stays

Rust: `cargo machete` (clean; `md-5` and `zxing-cpp` are imported as `md5`/`zxingcpp`, listed as ignored in
`crates/arcade-core/Cargo.toml`) plus `clippy -D warnings` for dead items. Frontend: `npx knip@5` with
`apps/desktop/frontend/knip.json`, whose entries are the HTML pages' scripts (`src/main.ts`, `src/pin.ts`) and the
test fixture `tests/visual.ts` that `tests/*.mjs` load; `src/design/` is vendored from Link and ignored. Removed:
`ShortcutClash.svelte` (no importer), `runTool`/`initials`, the `run_tool` Tauri command (the frontend runs every
tool as a job), and the `ToolStatus` type; types used only in their own file are no longer exported.

## D12 — Untangle (step 2): the desktop crate's root holds only what everything shares

`apps/desktop/src-tauri/src/lib.rs` was 2,542 lines with four reasons to change: the Tauri commands, the Island
window (visibility, placement, fitting, input region, release, and the Link/tool opens that wait for it), the tray
(native, recovery while Tools hosts the icon, and the host watcher) and the resident process (restart, predecessor,
second instance, `--status`). Each of the last three had its own globals in the root, so a change to one meant
reading all of it. They are now `island.rs`, `tray.rs` and `instance.rs`, each owning its state; `lib.rs` keeps the
commands, `run()` and the few helpers every module uses (`OS_ID`, `in_box_folder`). A pure move: no behaviour
changed (compiler-checked; desktop tests and the Island key/layout checks unchanged). The commands then moved to
`commands/<feature>.rs` (shortcut, tools, plugins, pipelines, engines, files, screen); `run()` registers them by path.
`lib.rs` went from 2,542 to 470 lines.

Copies in `arcade-core` that had to change together but didn't say so, now one each:
- `safe_stem` (output names after the input) existed three times. Two capped the stem at 100 characters and the
  PDF one at 100 bytes; a 100-character CJK or emoji stem is 300–400 bytes, past the 255-byte file name limit, so
  saving such a result could fail. One `tool_kit::safe_stem` with the byte cap (test
  `safe_stem_caps_bytes_not_characters`).
- `rgb_to_hsl` existed three times (colour converter, Pick screen colour, image palette): one in
  `converter_tools::color`, the converter's rounded `to_hsl` built on it.
- `barcode.rs` had its own copy of `tool_kit::option_str`.
- The set of characters a file name may not contain was written out six times (output stems, media names, renames,
  downloads, PDF names, the portable-name check). One `artifacts::unsafe_in_file_name`.
- How many files Compare text takes (2 to compare, 3 to merge, in order) was written out four times across
  `box.svelte.ts` and `IslandTool.svelte`, beside a `pdf.merge` special case the catalog already covered
  (`ui.input.kind: files`). One `fileList(tool)` in `box.svelte.ts` answers it for every tool.
- `arcade-core/src/lib.rs` kept the pipeline helpers (`validate_pipeline_id`, `humanize_id`, `pipeline_manifest`)
  and the catalog and request preparation (`builtin_catalog`, `input_compatible`, `fill_ui_defaults`,
  `text_options_from_numbers`) beside the `Arcade` runtime. They are in `pipeline.rs` and a new `catalog.rs`;
  `lib.rs` 1,023 -> 767 lines. The U-1 doc comment, which had slid onto `fill_ui_defaults`, is back on
  `text_options_from_numbers`.
- `box.svelte.ts` had its own `formatBytes` (1024-based, two decimals: "1.18 MB") and `formatDuration` beside the
  design kit's `formatSize`/`formatDuration`, which every app shares through `format-cases.json` ("1.2 MB"). Box's
  now only parse their input and call the kit's, so a size reads the same in Box as in Tools or Shelf.

## D13 — CI (step 6): one package per pull request, caches that fit

PR CI took about 40 minutes on Ubuntu and Windows: each PR built all three installers, and no Linux Rust cache
existed. The repository's 10 GB of caches held Windows (5.2 GB) and two macOS entries (3.1 GB each) plus the
Hyprland image, so Linux's was evicted and every Ubuntu run compiled from scratch. Now (Link spec §4) a pull request
builds one Linux package; Windows and macOS run every check and build their installers on main and in the release
workflow. On main, Windows and macOS drop their release build before the cache is saved, so the three OS caches fit.

## D14 — Memory (step 5): the resident gives back what windows leave, and runs unpacked

`AUDIT/memory.md` items for Box, with Tools' implementation (Tools D10, D18) as the model; nothing changes how Box
feels, and no timer runs while Box has no window (`apps/desktop/src-tauri/src/memory.rs`).
- **U1:** a resident start from the AppImage runs from its unpacked copy under `<data>/arcade/runtime/`
  (`runtime::relaunch_unpacked`), so no FUSE helper stays resident and pages come from the real disk; `$APPIMAGE`
  still names the installed file. `--quit`, `--hide` and `--restart` never start a resident and skip the copy; a
  copy offering to install itself keeps running from its mount. The package gate's UI check runs to an exit code,
  so its launcher waits for the copy. Package tests now follow the resident (Link endpoint, `--status`, the
  unpacked executable) rather than the launcher, which exits 0 at once (`verify-appimage.py` checks the copy runs).
- **W1 + W3b:** 3 s after any window is destroyed, if none is left, WebKit's network process ends (the next window
  starts a new one) and the idle heap is paged out (free unless memory is short); with a window left, a trim. This
  replaces the Island's trim-only release and also covers the dashboard and pinned captures.
- **W0:** the same release 15 s after the resident starts, and a trim after a Link request served while hidden.
- **OPEN1:** while a window is open, Box trims itself 2 s after a page loads and every 20 s; WebKit's helper
  processes get the same from `libarcade-trim.so` (vendored from Link v1.0.7, built by `prepare-bundle.py`,
  preloaded from `$APPDIR/usr/lib`, checked by `verify-appimage.py`).
- **BX1:** job results were already bounded (100 finished jobs, 32 early Link updates). The clipboard history was
  not: 500 items with images of up to 8 MB of RGBA each (10.7 MB as base64), all in memory. It now also keeps at
  most 64 MiB of text and image data, oldest unpinned first (`history_stays_within_its_byte_budget`).
- Compositing stays on (spec: the Island must stay smooth); the Island's 15 s keep-warm stays (hot re-show).

## D15 — Release machinery (step 6): candidates on the test channel, promotion to stable

`release.yml` ran on every pushed `v*` tag and published it as the latest stable release, so the first
`v1.0.0-rc.1` tag would have reached every user untested (PLAN.md rule zero). It now has Tools' shape (Link spec
§CI): `tag.yml` makes only `vX.Y.Z-rc.N` tags and starts Release; `release.yml` builds and verifies through
`ci.yml` and publishes through Link's reusable `app-publish` on the hidden `test` channel; `promote.yml` (Link's
`app-promote`) republishes a verified candidate's bytes as stable with a signed manifest; `nightly.yml` (Link's
`app-nightly`) republishes main CI's tested installers each night. The artifacts keep their `arcade-box-*` names,
which Link's real-fleet e2e picks up. Box's own `arcade-release.py` publish step and its rolling `nightly` release
are retired (the vendored generator goes in step 4's follow-up if nothing else uses it).

Dependabot (same step): each merge to main made it rebase every open Dependabot pull request, and each rebase
started a full three-OS build; on 2026-11-14 that queued 24 builds across the fleet ahead of release jobs. Box's
updates are now grouped per ecosystem (minor and patch together, majors separate) with `rebase-strategy: disabled`.

## D16 — Link v1.0.8

Box moves from Link v1.0.7 to v1.0.8 so the fleet runs one Link version: the `arcade-link` crate pin (only that
package changes in `Cargo.lock`), the reusable `app-publish`, `app-promote` and `app-nightly` workflows with their
`link-ref`, and the Link runner that `ci.yml` checks out for the tray gate (was v0.4.0). The design kit, last synced at
v0.4.15, is re-synced at v1.0.8; the sync changes two lines of `arcade.css` (the font stack gains the colour-emoji families, and a
tab panel keeps its focus outline for keyboard focus). 1.0.8 adds the `windows-installer` input (`nsis`, the default,
or `inno`) to the publish and nightly workflows; Box keeps the default, which is its NSIS installer.

## D17 — The release manifest script goes (untangle)

D15 retired Box's own publish step and left the vendored generator for removal if nothing else used it. Nothing does:
only its tests, `VENDORED.md` and `docs/dev/packaging.md` named it. `scripts/arcade-release.py` and its test are
deleted and `docs/dev/packaging.md` no longer documents running it; Link's `app-publish` writes the release manifests.
`spec/arcade-release.schema.json` stays vendored.

## D18 — Changelog in Keep a Changelog format

`CHANGELOG.md` follows Keep a Changelog 1.1.0: `## [X.Y.Z] - date` from each release's tag, old bullets unchanged and
grouped by their first verb, and an `[Unreleased]` section for v1 written from these decisions.

## D19 — Round 5 (step 7): data files, notices, uninstall

From the Round 5 audit of Box (schema versions, downgrade, security, notices, uninstall). Link's FS-2: every data file
carries a schema version, migrations are tested from released versions, and an older build never loses or corrupts
a newer file.
- **`clipboard-history.json`** gets `schema` (no field = 1, the shape every release since 0.1.0 wrote, unchanged).
  Every field has a default, and unknown fields at the top and in each item are kept through a save. A newer schema
  keeps its number. A file Box cannot read is moved to `clipboard-history.json.unreadable-<seconds>` and Box starts
  with an empty history instead of failing to start. Tests: a v0.4.3 fixture round-trips, unknown fields survive,
  a newer schema keeps its number, unreadable and incompatible files are kept aside, missing fields default.
- **Plugin `installed.json`** keeps `deny_unknown_fields` (it records grants). A schema above 1 is refused with
  "made by a newer Arcade Box". The field is written only once it is above 1, because Box 0.4.x denies unknown fields:
  a v1 file stays byte-for-byte in the released shape.
- **Turned-off apps** (`link.disabled_peers` in the settings table): an unreadable value is logged once and kept
  until the person changes the list, instead of being overwritten with an empty one. The provider cache is a cache.
- **SQLite:** the migrations are unchanged since 0.1.0. New tests: a database built with the released migrations and
  real rows opens with the current code, and one with a newer migration and an extra table opens untouched.
- **Notices:** `THIRD_PARTY_NOTICES.md` and `LICENSE` are bundled with every package (Tauri resources), and Settings →
  About has "Third-party notices", which opens the bundled file. `verify-appimage.py` checks the image contains it.
- **Uninstall:** Box's own unpacked copies (`arcade/runtime/box-*`, D14 U1) are removed on every uninstall; other
  apps' copies stay. Link's `remove_unpacked_except` would remove every app's, so Box does not call it.
  `verify-appimage.py` now compares the profile before install with what is left after `--uninstall --silent`
  (only Box's kept data may remain) and after a second install and `--uninstall --silent --remove-data` (nothing
  of Box's may remain; WebKit storage outside Box's folders is reported, not failed).
- Not done here, and still open for Round 5: N-1 against the previous peers and the gates on the published rc
  (both in D20), the downloader rows in SECURITY.md, accessibility checks, and README screenshots.

## D20 — Gates on the published rc

`gates.yml` runs every PLAN exit gate on a published `vX.Y.Z-rc.N`, never on a local build, through Link's reusable
`app-gates.yml@v1.0.9`: the rc and the previous stable release (v0.4.3) download and match `SHA256SUMS.txt` and the
rc's test-channel manifest; conformance on the repository and the published AppImage; N-1 (the Link CLIs the
released peers ship see the rc, read its status and describe it); background memory and the performance budgets;
the delta from v0.4.3 through Tools' updater, with the signed manifest and a tampered chunk; the NSIS and DMG smoke on
the published installers. Box adds its Linux checks on the published AppImage (notices, install/repair/uninstall
inventory, the upgrade from v0.4.3 with a user's data, Island close/reopen keeping the process, tray registration)
and Link's desktop suites on the same file (Tier 1 Hyprland blocks on any failure; Tier 2 sway, KWin, Mutter and X11).
`promote.yml` is run only after a green Gates run on the rc it promotes.

## D21 — Accessibility pass (step 7)

`tests/a11y.mjs` (in `scripts/check`, so in CI) runs axe-core's WCAG 2.1 A and AA rules on the Island (empty, search,
tool, first run) and on every dashboard page in light and dark, checks that every control Tab reaches on a dashboard
page looks different focused, and that nothing moves for longer than 10 ms when reduced motion is requested. Text at
125% is covered by `tests/layout.mjs`. Contrast passed already. On the sources before this change it found 34 problems,
all fixed:
- **Switches, checkboxes and sliders had no focus ring.** A rule meant for text fields (which show focus with their
  border) removed the outline from every `input`. It now applies to text-like inputs only; the others get the ring.
- **Category buttons had no name in the narrow sidebar** (≤ 760 px hides their label). They carry the category as
  their name and tooltip.
- **The search listbox held buttons.** A listbox may own only options; each result row's star sat inside it. The star
  stays as a pointer shortcut but is hidden from assistive technology and Tab; the keyboard way to change a favourite
  is the star in the tool's own view.
- **Names that did not contain the visible text** (WCAG 2.5.3, label in name): the footer's Esc button ("Esc Clear"
  was named "Clear the search") and the shortcut recorder. Their names now start with the visible words, with the
  rest in visually hidden text; the Esc button announces its key with `aria-keyshortcuts`.

README and user-guide screenshots (same step): they dated from 0.3 (2026-10-28) and no longer showed the Island's
footer or the current tool view. `npm run screenshots` (`tests/screenshots.mjs`) now takes all six from the visual
fixture at 2x in light and dark, so they can be retaken after any visible change.

## D22 — Link 1.0.11 for rc.2

`v1.0.0-rc.1`'s Gates stopped at "Published files": Tauri names the files after the product name
(`Arcade Box_1.0.0_amd64.AppImage`), GitHub stored them as `Arcade.Box_...`, and `SHA256SUMS.txt`, `arcade-release.json`
and `manifest.json` named the original, so neither the gates nor Tools could have fetched them. Box's 0.x release
workflow replaced spaces with `-`; moving to Link's `app-publish.yml` (D15) lost that. Link 1.0.11 fixes it for every
app (spaces become `-`, other characters are refused, the published file names must equal the metadata's). rc.1 is
not promoted; rc.2 is built from this change.

Link 1.0.8 → 1.0.11, what touches Box:
- `ARCADE_UNPACKED` now names the copy's folder, and a value that does not contain the running executable is
  dropped (1.0.11). With 1.0.8 a Restart's successor inherited `ARCADE_UNPACKED=1` from the copy and stayed on the
  FUSE mount. `scripts/verify-resident.py` now runs without the harness's `ARCADE_UNPACKED` and requires the
  resident and Restart's successor to run from `<data>/arcade/runtime/box-*`.
- `install::uninstall` removes the app's own unpacked copies (1.0.10). Box's own `remove_unpacked_copies` is gone;
  the copies under the user's XDG data folder (not the environment's when `ARCADE_HOME` redirects the install) go
  through Link's `runtime::remove_unpacked_of`. The installer test still checks that Box's copies go and Tools' stay.
- 1.0.9 adds `app-gates.yml` (already used by D20) and the harness's `ARCADE_UNPACKED=1`; the design kit's web files
  did not change (`scripts/sync-design.py` records v1.0.11 with the same hashes).
Every Link workflow and checkout Box uses is pinned to `v1.0.11`.

## D23 — Link 1.0.12's workflows for rc.3

`v1.0.0-rc.2`'s [Gates](https://github.com/qa-p1/Arcade-box/actions/runs/43921442645) passed every job on the
published files (files and manifest, budgets, delta, N-1, Windows and macOS smoke) except "Linux (conformance)":
`arcade-link: /lib/x86_64-linux-gnu/libc.so.6: version 'GLIBC_2.39' not found`. Link's `setup-arcade-link` cached the
CLI under `runner.os` and Link's commit only, so the ubuntu-22.04 gates job restored the CLI that Box's CI on main had
built on ubuntu-24.04. Box's code and the rc.2 files were not at fault, but the gates run from the rc tag's own
workflow file, so the fix needs a new rc. Link 1.0.12 keys the cache by runner image (and its harness accepts a
one-pixel cursor difference).

Every reusable Link workflow and `link-ref` Box uses is pinned to `v1.0.12`. The `arcade-link` crate, the build's Link
checkout and the vendored design kit stay at `v1.0.11`: 1.0.12 changes no Rust code and no design file, so the app
built for rc.3 is the rc.2 app. rc.2 is not promoted.
