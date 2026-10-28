#!/usr/bin/env python3
"""One version for Arcade Box.

The source is `[workspace.package] version` in the root Cargo.toml. The CLI
and the desktop app inherit it, Tauri reads it from the desktop crate and the
frontend build reads it from Cargo.toml (vite.config.ts), so no other file
repeats it. docs/user/shortcuts.json records the version its shortcuts were
checked against. This check fails when any of that drifts.

  python3 scripts/check-version.py            # fail on drift
  python3 scripts/check-version.py --print    # print the version
  python3 scripts/check-version.py --set 1.2.3
"""
import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CARGO = ROOT / 'Cargo.toml'
INHERIT = [ROOT / 'apps' / 'cli' / 'Cargo.toml', ROOT / 'apps' / 'desktop' / 'src-tauri' / 'Cargo.toml']
NO_VERSION = [
    ROOT / 'apps' / 'desktop' / 'src-tauri' / 'tauri.conf.json',
    ROOT / 'apps' / 'desktop' / 'src-tauri' / 'tauri.bundle.conf.json',
    ROOT / 'apps' / 'desktop' / 'frontend' / 'package.json',
]
LOCK = ROOT / 'apps' / 'desktop' / 'frontend' / 'package-lock.json'
SHORTCUTS = ROOT / 'docs' / 'user' / 'shortcuts.json'
SEMVER = re.compile(r'^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$')


def rel(path: Path) -> str:
    return path.relative_to(ROOT).as_posix()


def source() -> str:
    return tomllib.loads(CARGO.read_text(encoding='utf-8'))['workspace']['package']['version']


def problems() -> list[str]:
    version = source()
    found = []
    if not SEMVER.match(version):
        found.append(f'Cargo.toml [workspace.package] version {version!r} is not a semver version')
    for path in INHERIT:
        value = tomllib.loads(path.read_text(encoding='utf-8'))['package'].get('version')
        if value != {'workspace': True}:
            found.append(f'{rel(path)}: use version.workspace = true, not {value!r}')
    for path in NO_VERSION:
        if path.exists() and 'version' in json.loads(path.read_text(encoding='utf-8')):
            found.append(f'{rel(path)}: remove "version"; Cargo.toml [workspace.package] is the one source')
    lock = json.loads(LOCK.read_text(encoding='utf-8'))
    if 'version' in lock or 'version' in lock.get('packages', {}).get('', {}):
        found.append(f'{rel(LOCK)}: remove the root "version" (run npm install after removing it from package.json)')
    shortcuts = json.loads(SHORTCUTS.read_text(encoding='utf-8')).get('version')
    if shortcuts != version:
        found.append(f'{rel(SHORTCUTS)} says {shortcuts!r}; Cargo.toml [workspace.package] says {version!r}')
    return found


def set_version(version: str) -> None:
    if not SEMVER.match(version):
        sys.exit(f'{version!r} is not a semver version')
    text = CARGO.read_text(encoding='utf-8')
    text, done = re.subn(r'(?m)(^\[workspace\.package\]\n(?:[^\[\n].*\n)*?version\s*=\s*")[^"]*(")',
                         rf'\g<1>{version}\g<2>', text, count=1)
    if done != 1:
        sys.exit('Cargo.toml: no [workspace.package] version')
    CARGO.write_text(text, encoding='utf-8')
    text = SHORTCUTS.read_text(encoding='utf-8')
    text, done = re.subn(r'("version":\s*")[^"]*(")', rf'\g<1>{version}\g<2>', text, count=1)
    if done != 1:
        sys.exit(f'{rel(SHORTCUTS)}: no "version" field')
    SHORTCUTS.write_text(text, encoding='utf-8')
    print(f'Set {version}. Run cargo update -w to refresh Cargo.lock.')


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    group = parser.add_mutually_exclusive_group()
    group.add_argument('--print', action='store_true', help='print the version')
    group.add_argument('--set', metavar='VERSION', help='write VERSION to Cargo.toml and shortcuts.json')
    args = parser.parse_args()
    if args.set:
        set_version(args.set)
    found = problems()
    if found:
        print('Version drift:\n  ' + '\n  '.join(found), file=sys.stderr)
        return 1
    if args.print:
        print(source())
    elif not args.set:
        print(f'Version {source()} everywhere.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
