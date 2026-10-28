# Testing

`scripts/check` runs everything CI checks, in the same order; run it before
pushing. Each step can also run on its own:

| Check | Command |
|---|---|
| Rust format | `cargo fmt --all --check` |
| Rust lint (no warnings) | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| Rust tests, in an isolated profile | `python3 scripts/isolate.py cargo test --workspace --locked` |
| Frontend types | `npm run check` (in `apps/desktop/frontend`) |
| Frontend unit tests | `npm test` |
| Frontend build | `npm run build` |
| Undefined CSS variables | `python3 scripts/check-css-vars.py` (after the build) |
| Layout: nothing overflows | `npm run test:layout` |
| One version | `python3 scripts/check-version.py` |
| Docs links, shortcut data, generated tables | `python3 scripts/check-docs.py` |
| Vendored Link files | `python3 scripts/check-vendored.py` |
| Tool matrix and licences up to date | `python3 catalog/render_tool_matrix.py --check`, `python3 scripts/license_inventory.py --check` |

## Isolation

`scripts/isolate.py` runs a command with `HOME`, the XDG folders and
`ARCADE_HOME` inside a temporary folder, so tests never read or change your
real profile. Box keeps its files in Link's layout under `ARCADE_HOME`
(`.local/share/arcade/box`), the same layout Arcade Tools uses.

## The layout check

`tests/layout.mjs` loads the visual fixture (`tests/visual.html`) in
Chromium and fails when any element is wider than its box: the Island in
compact, search, tool and first-run modes at 320×116 and 740×740, and every
dashboard page at 720×520, 1100×760 and 1600×1000, in light and dark.
Scrolling containers and ellipsised text are allowed. Set
`PLAYWRIGHT_CHROMIUM` to use a Chromium that is already installed.

## Packaged builds

CI also bundles each installer and checks it in a disposable profile:
`scripts/verify-appimage.py`, `scripts/verify-nsis.ps1` and
`scripts/verify-dmg.sh` install, repair and uninstall the real package. See
[packaging](packaging.md) and [release](release.md).
