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
    parser.add_argument('--target', help='Rust target triple or universal-apple-darwin (default: native host)')
    args = parser.parse_args()
    version = subprocess.check_output(['rustc', '-vV'], text=True)
    host = next(line.removeprefix('host: ') for line in version.splitlines() if line.startswith('host: '))
    target = args.target or host
    env = dict(os.environ, CARGO_BUILD_JOBS='3')
    targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'] if target == 'universal-apple-darwin' else [args.target]
    destination = ROOT / 'apps/desktop/src-tauri/binaries'
    destination.mkdir(parents=True, exist_ok=True)
    names = ('arcade-box', 'arcade-plugin-worker')
    for rust_target in targets:
        command = ['cargo', 'build', '--release', '--locked', '-p', 'arcadebox', '--bin', 'arcade-box', '-p', 'arcade-plugin-host', '--bin', 'arcade-plugin-worker']
        if rust_target:
            command += ['--target', rust_target]
        subprocess.run(command, cwd=ROOT, env=env, check=True)
        build = Path(env.get('CARGO_TARGET_DIR', str(ROOT / 'target')))
        if not build.is_absolute():
            build = ROOT / build
        if rust_target:
            build /= rust_target
        build /= 'release'
        triple = rust_target or host
        extension = '.exe' if 'windows' in triple else ''
        for name in names:
            source = build / (name + extension)
            staged = destination / f'{name}-{triple}{extension}'
            shutil.copy2(source, staged)
            print(f'Staged {staged.relative_to(ROOT)}')
    if target == 'universal-apple-darwin':
        # Tauri also needs universal sidecars; an ARM-only worker would fail on Intel.
        for name in names:
            staged = destination / f'{name}-{target}'
            subprocess.run(['lipo', '-create', '-output', str(staged),
                            *(str(destination / f'{name}-{triple}') for triple in targets)], check=True)
            print(f'Staged {staged.relative_to(ROOT)}')


if __name__ == '__main__':
    main()
