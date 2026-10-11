#!/usr/bin/env python3
"""Check checksum-pinned Arcade Link files copied into this repository."""

from __future__ import annotations

import hashlib
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
EXPECTED = {
    "assets/strings/install.json": "2943f9f8459a0c2299bf7650a4beeb812b88d5097624483fdc252dc9d4342829",
    "packaging/windows/README.md": "4a28dbdb51f57581d74b8bcbde3cf265e087387a73a762d413d0033fb30c3c63",
    "packaging/windows/arcade.iss": "8a3419c09b0f0a4ca16cac91420512d4ed450ed968e65011421dfc5a46ee2ae8",
    "packaging/windows/arcade.nsh": "89e8561134fbde1a15fd0fbc7aba675d2f0092a0d84bee4048bef4732bed6287",
    "packaging/windows/arcade-integration.ps1": "a7c25ed6a98a7bae0a2c250fbdb9e7152f085809313210d12ae1e72e24ee53b4",
    "scripts/arcade-release.py": "8f360855419930730c486d0978acaecf7a8179cc063465c9f8324878dcf6c2fd",
    "scripts/validate_shortcuts.py": "3b5d2981ab87fdf07437daa294dce825d59b63580430e9ffa2b58e87d5d72cf0",
    "spec/vectors/accelerators.json": "67d0d5a86e8d9f953c2cf779136c790bdd62d7983fa12d36a65c446e12b246ec",
}


def main() -> int:
    errors = []
    for relative, expected in EXPECTED.items():
        path = ROOT / relative
        actual = hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else "missing"
        if actual != expected:
            errors.append(f"{relative}: expected {expected}, got {actual}")
    if errors:
        print("Vendored Arcade Link assets are stale:\n" + "\n".join(errors), file=sys.stderr)
        return 1
    print(f"Verified {len(EXPECTED)} vendored Arcade Link assets")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
