#!/usr/bin/env python3
"""Run the normal Box desktop alone or with real peers inside tools/e2e.py run.

Never invoke this directly in a live desktop. The outer runner owns Xvfb and
D-Bus; this script tracks and stops only the app PIDs it starts.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import runpy
import sqlite3
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
LINK = ROOT.parent / 'Arcade-link'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--peers', action='store_true')
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/arcade-desktop')
    args = parser.parse_args()
    assert os.environ.get('ARCADE_E2E_INNER') == '1', 'Use tools/e2e.py run -- to isolate this GUI run'
    root = Path(os.environ['ARCADE_E2E_ROOT']).resolve()
    assert Path(os.environ['HOME']).resolve().is_relative_to(root)
    assert not os.environ.get('WAYLAND_DISPLAY') and not os.environ.get('HYPRLAND_INSTANCE_SIGNATURE')
    spec = importlib.util.spec_from_file_location('arcade_e2e', LINK / 'tools/e2e.py')
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    # Use the runner's helpers with the already-created private display/bus.
    session = runner.Session.__new__(runner.Session)
    session.root, session.env, session.procs = root, dict(os.environ), {}
    session.env['WEBKIT_DISABLE_COMPOSITING_MODE'] = '1'
    runner.APPS['arcade.box']['bin'] = str(args.binary.resolve())
    helpers = runpy.run_path(str(LINK / 'tools/e2e_checks/box.py'), init_globals={
        'check': lambda *groups: lambda function: function,
        'APPS': runner.APPS, 'CLI': runner.CLI, 'Session': runner.Session,
    })
    try:
        # Normal one-shot initialization has no UI, listener or manifest.
        initialized = subprocess.run([str(args.binary), '--arcade-invoke'], env=session.env,
            input=json.dumps({'v':1,'id':1,'method':'invoke','params':{'action':'box.pipelines'}})+'\n',
            capture_output=True, text=True, timeout=20)
        assert initialized.returncode == 0, initialized.stdout + initialized.stderr
        db = Path(session.env['XDG_DATA_HOME']) / 'dev.arcadebox.app/arcade.sqlite3'
        with sqlite3.connect(db) as connection:
            connection.execute("INSERT OR REPLACE INTO settings(key,value) VALUES('onboarding_complete','true')")
        if args.peers:
            helpers['clipboard_mesh'](session)
            for app in ('arcade.lens', 'arcade.look', 'arcade.wheel'):
                session.start(app)
        runner.APPS['arcade.box']['args'] = []
        session.start('arcade.box')
        helpers['resident'](session)
        wire = helpers['Wire'](session)
        try:
            status = wire.call('app.status', {})['result']
            assert status['status']['mode'] == 'foreground', status
        finally:
            wire.close()
        expected = {'arcade.box', 'arcade.lens', 'arcade.look', 'arcade.wheel', 'arcade.clipboard'} if args.peers else {'arcade.box'}
        installed = {row['id'] for row in json.loads(session.cli('ls','--json',check=True).stdout)}
        assert installed == expected, installed
        # Open the Island through its real shortcut, then run a real image tool.
        visible = session.xdotool('search', '--onlyvisible', '--name', '^Arcade Box$')
        if visible:
            session.xdotool('windowfocus', '--sync', visible.splitlines()[0], 'key', 'Escape')
            time.sleep(.5)
        session.xdotool('key', 'ctrl+alt+space')
        session.wait_window('Arcade Box')
        source = helpers['png'](root / 'standalone-source.png', 240, 160)
        window = helpers['open_tool'](session, 'arcade.image.convert', [{'type':'file/image','path':str(source)}], {'format':'webp','quality':82})
        helpers['run_open_tool'](session)
        if args.peers:
            helpers['ui_wait'](session, 'Send to my devices')
            helpers['ui_wait'](session, 'Preview')
        else:
            labels = ' '.join(line['text'] for line in helpers['ui_lines'](session)).lower()
            assert 'send to my devices' not in labels and 'add to wheel' not in labels and 'preview' not in labels, labels
        build = '-debug' if args.binary.resolve().parent.name == 'debug' else ''
        shot = session.screenshot(f'box-normal-desktop{build}-' + ('all-peers' if args.peers else 'alone'), window)
        assert session.procs['arcade.box'].poll() is None
        print('PASS normal desktop', 'all peers' if args.peers else 'alone', args.binary, 'shortcut → image conversion → result; foreground mode; screenshot', shot)
    finally:
        for process in reversed(list(session.procs.values())):
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


if __name__ == '__main__':
    main()
