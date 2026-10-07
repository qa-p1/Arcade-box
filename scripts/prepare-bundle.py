#!/usr/bin/env python3
"""Build and stage the canonical CLI and trusted plugin worker for Tauri.

Only installer builds use tauri.bundle.conf.json. Ordinary cargo builds do
not require generated sidecars. No install, launch or profile changes occur.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', help='Rust target triple (default: native host)')
    args = parser.parse_args()
    host = subprocess.check_output(['rustc', '--print', 'host-tuple'], text=True).strip()
    target = args.target or host
    env = dict(os.environ, CARGO_BUILD_JOBS='3')
    command = ['cargo', 'build', '--release', '--locked', '-p', 'arcadebox', '--bin', 'arcade-box', '-p', 'arcade-plugin-host', '--bin', 'arcade-plugin-worker']
    if args.target:
        command += ['--target', args.target]
    subprocess.run(command, cwd=ROOT, env=env, check=True)
    build = Path(env.get('CARGO_TARGET_DIR', str(ROOT / 'target')))
    if not build.is_absolute():
        build = ROOT / build
    if args.target:
        build /= args.target
    build /= 'release'
    destination = ROOT / 'apps/desktop/src-tauri/binaries'
    destination.mkdir(parents=True, exist_ok=True)
    extension = '.exe' if 'windows' in target else ''
    for name in ('arcade-box', 'arcade-plugin-worker'):
        source = build / (name + extension)
        staged = destination / f'{name}-{target}{extension}'
        shutil.copy2(source, staged)
        print(f'Staged {staged.relative_to(ROOT)}')


if __name__ == '__main__':
    main()
