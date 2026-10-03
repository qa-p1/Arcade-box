#!/usr/bin/env python3
"""Install a source build at a durable location and register its portal app ID.

No shell profiles, autostart entries, or compositor configuration are changed.
"""
from __future__ import annotations

import datetime
import difflib
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent
DATA = Path(os.environ.get('XDG_DATA_HOME', str(Path.home() / '.local/share'))).resolve()
INSTALL = DATA / 'arcade-box/local-build'
ENTRY = DATA / 'applications/dev.arcadebox.app.desktop'
MARKER = 'X-ArcadeBox-LocalBuild=true'


def durable(path: Path) -> None:
    if any(path == root or root in path.parents for root in (Path('/tmp'), Path('/var/tmp'), Path('/run'))):
        raise SystemExit(f'Refusing to register a desktop application in a temporary location: {path}')
    if any(character in str(path) for character in '\n\r\x00='):
        raise SystemExit('The installation path contains characters unsupported by desktop entries.')


def string_value(value: str) -> str:
    return value.replace('\\', '\\\\').replace('\n', '\\n').replace('\r', '\\r').replace('\t', '\\t')


def exec_argument(path: Path) -> str:
    value = str(path).replace('%', '%%')
    for character in ('\\', '"', '`', '$'):
        value = value.replace(character, '\\' + character)
    return string_value('"' + value + '"')


def install_file(source: Path, destination: Path, executable: bool = False) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    staging = destination.with_name(destination.name + '.new')
    try:
        # Strip debug sections from the installed copy only; keep build symbols intact.
        strip = shutil.which('strip') if executable else None
        if strip:
            subprocess.run([strip, '--strip-debug', '-o', str(staging), str(source)], check=True)
        else:
            shutil.copyfile(source, staging)
        staging.chmod(0o755 if executable else 0o644)
        staging.replace(destination)
    finally:
        staging.unlink(missing_ok=True)


def main() -> None:
    durable(INSTALL)
    # Read existing persistent registration before making any change.
    if ENTRY.is_symlink():
        raise SystemExit(f'Refusing to replace a symlink desktop entry: {ENTRY}')
    existing = ENTRY.read_text() if ENTRY.exists() else None
    for name in ('arcade-desktop', 'arcade-plugin-worker'):
        source = ROOT / 'target/debug' / name
        if not source.is_file():
            raise SystemExit(f'Build {name} first with scripts/run-desktop.sh')
    for name in ('arcade-desktop', 'arcade-plugin-worker'):
        install_file(ROOT / 'target/debug' / name, INSTALL / 'bin' / name, True)
    icon = INSTALL / 'icon.png'
    install_file(ROOT / 'apps/desktop/src-tauri/icons/icon.png', icon)
    content = '\n'.join([
        '[Desktop Entry]', 'Type=Application', 'Name=Arcade Box',
        'Comment=Tools for everyday file, text, and media tasks',
        f'Exec={exec_argument(INSTALL / "bin/arcade-desktop")}',
        f'Icon={string_value(str(icon))}', 'Terminal=false', 'Categories=Utility;',
        'StartupNotify=false', 'StartupWMClass=arcade-desktop', MARKER, '',
    ])
    if existing is not None and MARKER not in existing:
        print(f'Keeping your existing desktop registration: {ENTRY}')
        return
    if existing == content:
        print(f'Updated local build: {INSTALL}')
        return
    ENTRY.parent.mkdir(parents=True, exist_ok=True)
    staging = ENTRY.with_name('.dev.arcadebox.app.new.desktop')
    staging.write_text(content)
    try:
        validator = shutil.which('desktop-file-validate')
        if validator:
            subprocess.run([validator, str(staging)], check=True)
        if existing is not None:
            stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
            backup = ENTRY.with_suffix(f'.desktop.backup-{stamp}')
            shutil.copy2(ENTRY, backup)
            print(f'Previous desktop entry backed up: {backup}')
        print(''.join(difflib.unified_diff((existing or '').splitlines(True), content.splitlines(True), fromfile=str(ENTRY), tofile=str(ENTRY))), end='')
        staging.replace(ENTRY)
        if validator:
            subprocess.run([validator, str(ENTRY)], check=True)
        updater = shutil.which('update-desktop-database')
        if updater:
            subprocess.run([updater, str(ENTRY.parent)], check=True)
    finally:
        staging.unlink(missing_ok=True)
    print(f'Installed local build: {INSTALL}')


if __name__ == '__main__':
    main()
