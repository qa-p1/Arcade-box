#!/usr/bin/env python3
"""Verify the checksum inventory for unchanged Arcade Link sources, including
the design kit (scripts/sync-design.py): every file in the kit folder must be
recorded, so nothing there is edited or added by hand."""
import hashlib
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[1]
entries = re.findall(r"^([0-9a-f]{64})  (\S+)$", (root / "VENDORED.md").read_text(), re.M)
if not entries:
    sys.exit("Vendored inventory contains no checksums")
failed = [name for expected, name in entries
          if not (root / name).is_file() or hashlib.sha256((root / name).read_bytes()).hexdigest() != expected]
if failed:
    sys.exit("Vendored checksum drift: " + ", ".join(failed))
recorded = {name for _, name in entries}
kit = root / "apps/desktop/frontend/src/design"
unrecorded = sorted(p.relative_to(root).as_posix() for p in kit.rglob("*") if p.is_file()
                    and p.relative_to(root).as_posix() not in recorded)
if unrecorded:
    sys.exit("Not recorded in VENDORED.md (run scripts/sync-design.py): " + ", ".join(unrecorded))
print(f"Verified {len(entries)} vendored Arcade Link files")
