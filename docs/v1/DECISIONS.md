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
