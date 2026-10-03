#!/usr/bin/env python3
"""Record the locked Rust and npm dependency closure for human license review.

This is inventory, not an automatic legal compatibility verdict. Native
providers, ML models, bundled OS libraries, and final notices are audited
separately for each release artifact.
"""

import argparse
import json
import subprocess
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "licenses" / "resolved.json"


def cargo_packages() -> list[dict[str, str | None]]:
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--locked", "--format-version", "1"],
            cwd=ROOT,
        )
    )
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    checksums = {
        (package["name"], package["version"]): package.get("checksum")
        for package in lock["package"]
    }
    packages = []
    for package in metadata["packages"]:
        if not package.get("source"):
            continue
        packages.append(
            {
                "ecosystem": "cargo",
                "name": package["name"],
                "version": package["version"],
                "license": package.get("license"),
                "licenseFile": package.get("license_file"),
                "repository": package.get("repository"),
                "source": package["source"],
                "checksum": checksums.get((package["name"], package["version"])),
            }
        )
    return packages


def npm_packages() -> list[dict[str, str | None]]:
    lock = json.loads(
        (ROOT / "apps" / "desktop" / "frontend" / "package-lock.json").read_text(
            encoding="utf-8"
        )
    )
    packages = []
    for location, package in lock["packages"].items():
        if not location or package.get("link"):
            continue
        packages.append(
            {
                "ecosystem": "npm",
                "name": location.rsplit("node_modules/", 1)[-1],
                "version": package.get("version"),
                "license": package.get("license"),
                "repository": package.get("repository"),
                "source": package.get("resolved"),
                "checksum": package.get("integrity"),
            }
        )
    return packages


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if the committed inventory is stale")
    args = parser.parse_args()
    packages = sorted(
        cargo_packages() + npm_packages(),
        key=lambda package: (package["ecosystem"], package["name"], package["version"] or ""),
    )
    unresolved = [
        f"{package['ecosystem']}:{package['name']}@{package['version']}"
        for package in packages
        if not package.get("license") and not package.get("licenseFile")
    ]
    inventory = {
        "schemaVersion": 1,
        "purpose": "Locked third-party dependency inventory; human license and notice review remains required",
        "packageCount": len(packages),
        "unresolvedLicenseMetadata": unresolved,
        "packages": packages,
    }
    rendered = json.dumps(inventory, indent=2, ensure_ascii=False) + "\n"
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text(encoding="utf-8") != rendered:
            raise SystemExit("Resolved license inventory is stale; run python3 scripts/license_inventory.py")
    else:
        OUTPUT.write_text(rendered, encoding="utf-8")
        print(f"Wrote {len(packages)} packages; {len(unresolved)} have no license metadata")


if __name__ == "__main__":
    main()
