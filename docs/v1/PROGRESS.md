# Arcade Box v1 progress

Detail for `qa-p1/Arcade-catalog` › `dev/v1/PROGRESS.md`. Newest first.

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
