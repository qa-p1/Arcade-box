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
audit, while moving Box to Link 1.0.6 (`docs/v1/audit/box-real.md`). They are P1, fixed in step 3 with the other
P0/P1 fixes, each with a test that fails first; SECURITY.md says the CSP is missing until then rather than claim it.

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
  it against a GPL FFmpeg. MP4/H.264 previews need the host's plugins and say so. The screen recorder keeps using the
  host's GStreamer (it already clears the image's plugin paths). Cost: the image grows by about 15–25 MB.
- **Test:** `scripts/verify-appimage.py` (CI and Release) fails if the image lacks any of the 12 plugins playback
  needs.
