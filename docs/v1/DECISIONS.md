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
