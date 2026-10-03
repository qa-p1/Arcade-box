#!/usr/bin/env bash
set -euo pipefail

arcade_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$arcade_root"

arcade_build=true
arcade_foreground=false
for arcade_arg in "$@"; do
  case "$arcade_arg" in
    --no-build) arcade_build=false ;;
    --foreground) arcade_foreground=true ;;
    --help)
      echo 'Usage: ./scripts/run-desktop.sh [--no-build] [--foreground]'
      echo 'Build Arcade Box and launch it in the background tray. Use --foreground to keep logs in this terminal.'
      exit 0
      ;;
    *) echo "Unknown option: $arcade_arg" >&2; exit 2 ;;
  esac
done

if [[ "$arcade_build" == true ]]; then
  if [[ ! -d apps/desktop/frontend/node_modules ]]; then
    npm ci --prefix apps/desktop/frontend
  fi
  npm run build --prefix apps/desktop/frontend
  cargo build -p arcade-desktop -p arcade-plugin-host --features arcade-desktop/custom-protocol
fi

if [[ "$(uname -s)" == "Linux" ]]; then
  python3 scripts/install-local-desktop.py
  arcade_data="${XDG_DATA_HOME:-$HOME/.local/share}"
  arcade_binary="$arcade_data/arcade-box/local-build/bin/arcade-desktop"
else
  arcade_binary="$arcade_root/target/debug/arcade-desktop"
fi

if [[ "$arcade_foreground" == true ]]; then
  exec "$arcade_binary"
fi

arcade_state="${XDG_STATE_HOME:-$HOME/.local/state}/arcade-box"
mkdir -p "$arcade_state"
python3 - "$arcade_binary" "$arcade_state/desktop.log" <<'PYTHON'
import subprocess
import sys
import time

binary, log_path = sys.argv[1:]
with open(log_path, 'ab', buffering=0) as log:
    # Separate session, closed stdin, and durable logs: closing this terminal
    # must not terminate the resident app or leave output pipes open.
    process = subprocess.Popen([binary], stdin=subprocess.DEVNULL, stdout=log,
                               stderr=subprocess.STDOUT, start_new_session=True)
    time.sleep(1)
    status = process.poll()
    if status is None:
        print(f'Arcade Box is running in the background (PID {process.pid}).')
    elif status == 0:
        print('Arcade Box is already running. Use its shortcut or tray icon.')
    else:
        raise SystemExit(f'Arcade Box could not start (exit {status}). Check {log_path}')
print('Press your shortcut (default Ctrl+Alt+Space) to open the Island, or use the tray icon.')
print(f'Runtime log: {log_path}')
PYTHON
