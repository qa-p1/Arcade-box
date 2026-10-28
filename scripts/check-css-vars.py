#!/usr/bin/env python3
"""Fail when the built frontend uses a CSS variable nothing defines.

Reads every stylesheet Vite wrote to apps/desktop/frontend/dist (run
`npm run build` first) and the custom properties the components set from
script (`style:--name`, `setProperty('--name'`). A `var(--x, fallback)`
with a fallback is allowed; a bare `var(--x)` must be defined somewhere.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FRONTEND = ROOT / "apps/desktop/frontend"
dist = FRONTEND / "dist"
sheets = sorted(dist.rglob("*.css"))
if not sheets:
    sys.exit("check-css-vars: no built CSS; run `npm run build` in apps/desktop/frontend first")

css = "\n".join(path.read_text(encoding="utf-8") for path in sheets)
defined = set(re.findall(r"(--[\w-]+)\s*:", css))
for source in list((FRONTEND / "src").rglob("*.svelte")) + list((FRONTEND / "src").rglob("*.ts")):
    text = source.read_text(encoding="utf-8")
    defined.update(re.findall(r"style:(--[\w-]+)", text))
    defined.update(re.findall(r"setProperty\(\s*['\"](--[\w-]+)", text))

bare = set(re.findall(r"var\(\s*(--[\w-]+)\s*\)", css))
missing = sorted(bare - defined)
if missing:
    print("check-css-vars: used but never defined: " + ", ".join(missing), file=sys.stderr)
    sys.exit(1)
print(f"check-css-vars: {len(bare)} variables, all defined")
