#!/usr/bin/env python3
"""Smoke-test a real AppImage outside the checkout with a disposable profile.

Adapted from Arcade Look's check. CI only, not a real desktop.
"""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile


# Opens Settings, a webview, as the tray menu does.
WINDOW_ARGS = ("--settings",)


def unpacked_copies(root):
    """Processes running Box from an unpacked copy under `root` (Link U1)."""
    found = []
    for proc in Path("/proc").iterdir():
        try:
            exe = os.readlink(proc / "exe")
        except OSError:
            continue
        if exe.startswith(str(root) + "/") and "/arcade/runtime/box-" in exe and exe.endswith("/arcade-desktop"):
            found.append(int(proc.name))
    return found


def inventory(root):
    """Every file and symlink under `root`, relative to it (directories are not counted: Link leaves its empty
    parents behind, and empty folders hold nothing of the user's)."""
    found = set()
    for folder, dirs, files in os.walk(root):
        base = Path(folder)
        found.update((base / name).relative_to(root) for name in files)
        found.update((base / name).relative_to(root) for name in dirs if (base / name).is_symlink())
    return found


def leftovers(root, before, env, keep_user_data):
    """What an uninstall left that was not there before the install: (fails, webview). `keep_user_data` allows
    Box's own folders (`arcade/box`, under data, config and cache), which a plain uninstall keeps on purpose.
    Not counted: the work folder this script made, and Link's runtime state (sockets and locks in `run` and
    `handoff`, which the script's SIGKILL of the app leaves behind). WebKit's own storage (a folder next to
    Arcade's under the XDG roots, not one of Box's data folders) is only reported."""
    webview_roots = [Path(env[key]).relative_to(root) for key in ("XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_CONFIG_HOME")]
    runtime_state = {Path("run"), Path("arcade/run"), Path("arcade/handoff")}
    fails, webview = [], []
    for path in sorted(inventory(root) - before):
        parts = path.parts
        if parts[0] == "work" or any(path.is_relative_to(state) for state in runtime_state):
            continue
        if keep_user_data and any(parts[i:i + 2] == ("arcade", "box") for i in range(len(parts) - 1)):
            continue
        if any(path.is_relative_to(base) and path.relative_to(base).parts[:1] != ("arcade",) for base in webview_roots):
            webview.append(path)
            continue
        fails.append(path)
    return fails, webview


def survives(executable, env, work, root, *args, seconds=15):
    """Starts `executable` from the user's directory `work` and fails unless Box
    is still running `seconds` later from its unpacked copy under `root`; then
    stops it and everything it started. The launcher copies the image once to
    <data>/arcade/runtime/, starts Box from there and exits 0 (U1, D14), so a
    shell holds the session open after it."""
    work.mkdir(exist_ok=True)
    with tempfile.TemporaryFile() as log:
        child = subprocess.Popen(
            ["sh", "-c", '"$0" "$@" || exit $?; exec sleep 600', str(executable), *args],
            cwd=work, env=dict(env, OWD=str(work)),
            stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT, start_new_session=True,
        )
        unpacked = []
        try:
            code = child.wait(timeout=seconds)
        except subprocess.TimeoutExpired:
            code = None
            unpacked = unpacked_copies(root)
        finally:
            for pid in unpacked_copies(root):
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()
        if code is not None or not unpacked:
            log.seek(0)
            output = log.read().decode(errors="replace")[-4000:]
            if code is not None:
                raise RuntimeError(f"{executable} {' '.join(args)} exited {code} within {seconds}s:\n{output}")
            raise RuntimeError(f"{executable} {' '.join(args)} is not running from its unpacked copy (U1):\n{output}")


def main():
    image, app_id, filename, cli_name = sys.argv[1:]
    image = Path(image).resolve(strict=True)
    # Link treats executables anywhere under a .git tree as development builds.
    with tempfile.TemporaryDirectory(prefix=app_id + "-", dir=os.environ.get("RUNNER_TEMP")) as temporary:
        root = Path(temporary).resolve()
        for parent in (root, *root.parents):
            if (parent / ".git").exists():
                raise RuntimeError("AppImage verification must run outside the checkout")
        env = os.environ.copy()
        for key in ("APPIMAGE", "APPDIR", "ARCADE_DEV_BUILD"):
            env.pop(key, None)
        for key, directory in (
            ("HOME", "home"), ("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "run"), ("ARCADE_HOME", "arcade"),
        ):
            path = root / directory
            path.mkdir()
            env[key] = str(path)
        (root / "run").chmod(0o700)
        env["APPIMAGE_EXTRACT_AND_RUN"] = "1"
        env["ARCADE_NO_INSTALL_PROMPT"] = "1"
        # ARCADE_HOME redirects Link's Applications, integration and CLI roots.
        install_home = Path(env["ARCADE_HOME"])
        env["PATH"] = str(install_home / ".local/bin") + os.pathsep + env.get("PATH", "")
        download = root / "Downloaded.AppImage"
        shutil.copy2(image, download)
        download.chmod(0o755)

        def run(executable, *args):
            result = subprocess.run(
                [str(executable), *args], env=env, text=True, encoding="utf-8",
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=180,
            )
            if result.returncode:
                raise RuntimeError(f"{executable} {' '.join(args)} exited {result.returncode}\n{result.stdout}\n{result.stderr}")
            return result.stdout

        # BR-3: WebKit plays audio and video through GStreamer, and the image points GStreamer at its own plugin
        # folder only, so the plugins playback needs must be inside it (`bundleMediaFramework`, D9).
        extracted = root / "extracted"
        extracted.mkdir()
        subprocess.run([str(download), "--appimage-extract"], cwd=extracted, env=env, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=180)
        plugins = {p.name for p in (extracted / "squashfs-root").rglob("libgst*.so") if "gstreamer" in str(p.parent)}
        needed = {"libgstapp.so", "libgstcoreelements.so", "libgstplayback.so", "libgsttypefindfunctions.so",
                  "libgstaudioconvert.so", "libgstaudioresample.so", "libgstautodetect.so", "libgstpulseaudio.so",
                  "libgstvpx.so", "libgstmatroska.so", "libgstopus.so", "libgstvorbis.so"}
        assert needed <= plugins, f"GStreamer plugins missing from the image: {sorted(needed - plugins)} (has {len(plugins)})"
        # OPEN1 (D14): Box preloads the trimmer into WebKit's helper processes (memory.rs).
        assert (extracted / "squashfs-root/usr/lib/libarcade-trim.so").is_file(), "AppImage lacks libarcade-trim.so"
        # Third-party notices ship inside the image (a Tauri resource) so About can open them offline.
        notices = list((extracted / "squashfs-root").rglob("THIRD_PARTY_NOTICES.md"))
        assert notices and all(p.is_file() and p.stat().st_size > 0 for p in notices), "AppImage lacks THIRD_PARTY_NOTICES.md"
        shutil.rmtree(extracted)

        version = run(download, "--version").strip().split()[-1]
        # ARCADE_APP.md CLI-2: an unknown argument prints usage and exits 2, before anything starts.
        wrong = subprocess.run([str(download), "--no-such-flag"], env=env, text=True, capture_output=True, timeout=60)
        assert wrong.returncode == 2 and "--no-such-flag" in wrong.stderr and "--background" in wrong.stderr, (
            f"unknown argument: exit {wrong.returncode}\n{wrong.stdout}\n{wrong.stderr}")
        before = inventory(root)
        run(download, "--install", "--silent", "--no-launch")
        receipt_path = root / "arcade/installs" / (app_id + ".json")
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
        assert receipt["schema"] == 1 and receipt["id"] == app_id, receipt
        assert receipt["method"] == "appimage" and receipt["managedBy"] == "self", receipt
        assert receipt["version"] == version and receipt["channel"] == "stable", receipt
        installed = install_home / "Applications/Arcade" / filename
        assert Path(receipt["path"]) == installed and os.access(installed, os.X_OK), receipt
        assert installed.read_bytes() == image.read_bytes(), "Installed package differs from the download"
        integration = receipt["integration"]
        desktop = Path(integration["desktopEntry"])
        cli = Path(integration["cli"])
        # A fresh install starts at login, like every Arcade app.
        autostart = Path(integration["autostart"])
        assert autostart == install_home / ".config/autostart/arcade-box.desktop", integration
        icons = [Path(path) for path in integration["icons"]]
        entries = [installed, desktop, cli, autostart, *icons]
        assert icons, "No installed icons"
        for path in entries:
            assert path.is_relative_to(root) and path.exists(), path
        assert cli == install_home / ".local/bin" / cli_name and cli.is_symlink(), cli
        assert cli.resolve() == installed, cli
        assert f"X-Arcade-Id={app_id}" in desktop.read_text(encoding="utf-8"), desktop
        assert run(cli_name, "--version").strip().split()[-1] == version
        status = json.loads(run(cli_name, "--integration-status"))
        assert status["receipt"]["path"] == str(installed), status
        assert status["entries"] and all(entry["exists"] for entry in status["entries"]), status

        # The app itself must survive its first webview. AppRun changes into
        # $APPDIR/usr and the mounting runtime sets $OWD (extract-and-run
        # doesn't, so it is set here); WebKit finds its helpers relative to
        # $APPDIR/usr and aborts the whole app when they aren't there.
        survives(installed, env, root / "work", root, *WINDOW_ARGS)

        # Being alive is insufficient: exercise the packaged WebView's native
        # catalog, dashboard event permissions, Island handoff and calculation.
        checked = subprocess.run([str(installed), '--dashboard'],
            env=dict(env, ARCADE_UI_CHECK='1', OWD=str(root / 'work')),
            cwd=root / 'work', capture_output=True, text=True, timeout=60)
        ui_log = install_home / '.local/share/arcade/box/logs/arcade-box.log'
        report = checked.stdout + checked.stderr + (ui_log.read_text() if ui_log.exists() else '')
        assert checked.returncode == 0 and 'UI_CHECK passed' in report, report[-5000:]


        # A missing launcher must be repaired from the installed package.
        desktop.unlink()
        run(installed, "--repair")
        assert desktop.is_file(), "Repair did not restore the launcher"
        status = json.loads(run(installed, "--integration-status"))
        assert all(entry["exists"] for entry in status["entries"]), status
        # Box's data folder (arcade_core::dirs): Link's layout under ARCADE_HOME.
        app_home = install_home / ".local/share/arcade/box"
        app_home.mkdir(parents=True, exist_ok=True)
        sentinel = app_home / "keep-user-data.txt"
        sentinel.write_text("preserve", encoding="utf-8")
        run(installed, "--uninstall", "--silent")
        for path in [receipt_path, *entries]:
            assert not path.exists() and not path.is_symlink(), f"Uninstall retained {path}"
        assert sentinel.read_text(encoding="utf-8") == "preserve", "Uninstall removed user data"
        for name in (".profile", ".bash_profile", ".bashrc", ".zprofile", ".zshrc"):
            assert not (root / "home" / name).exists(), f"Installer created {name}"
        # Nothing but Box's kept data is new, and the unpacked runtime copies (U1) are gone.
        fails, webview = leftovers(root, before, env, keep_user_data=True)
        assert not fails, "Uninstall left files behind:\n" + "\n".join(map(str, fails))
        copies = list(root.glob("**/arcade/runtime/box-*"))
        assert not copies, f"Uninstall left unpacked copies: {copies}"
        # "Remove my data": install again, run Box once, then nothing of Box's remains, its data folder included.
        run(download, "--install", "--silent", "--no-launch")
        assert receipt_path.is_file(), "Second install wrote no receipt"
        run(installed, "--version")
        app_home.mkdir(parents=True, exist_ok=True)
        sentinel.write_text("remove", encoding="utf-8")
        run(installed, "--uninstall", "--silent", "--remove-data")
        for path in [receipt_path, *entries]:
            assert not path.exists() and not path.is_symlink(), f"Uninstall --remove-data retained {path}"
        assert not app_home.exists(), "Uninstall --remove-data kept Box's data folder"
        fails, webview_after = leftovers(root, before, env, keep_user_data=False)
        assert not fails, "Uninstall --remove-data left files behind:\n" + "\n".join(map(str, fails))
        copies = list(root.glob("**/arcade/runtime/box-*"))
        assert not copies, f"Uninstall --remove-data left unpacked copies: {copies}"
        for path in sorted({*webview, *webview_after}):
            print(f"note: WebView storage outside Box's data folders remains: {path}")
        print(f"{app_id}: AppImage install, receipt, launcher, icons, CLI, repair, uninstall and uninstall --remove-data passed (CI only, not a real desktop)")


if __name__ == "__main__":
    main()
