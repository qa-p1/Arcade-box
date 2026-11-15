#!/usr/bin/env python3
"""Upgrade from a published Box release to a new one, keeping the user's data (PLAN exit gate "Upgrade test").

    dbus-run-session -- xvfb-run -a python3 scripts/verify-upgrade.py OLD.AppImage NEW.AppImage

OLD is the previous release's published file (check it against its SHA256SUMS.txt first), NEW the candidate's. In a
private profile: OLD is installed from a download, which starts it at login like every fresh install, and it runs with
the data a user of it has: settings, favorites, usage and history, aliases, jobs, a saved pipeline, an installed
plugin, Link choices and clipboard history, in OLD's own file shapes (the pipeline and the plugin saved by OLD's own
`arcade-box` commands, the rest written into the tables and the file OLD created), then read by OLD itself. NEW is then
downloaded and installed over it while OLD runs, the way a user updates by hand. NEW must replace the installed copy
in place, stop OLD, start, keep every value of the data (a newer Box may add fields), keep Start at login, move the
launcher, commands and login entry to itself, leave no second launcher, and stop on --quit. NEW's own `arcade-box`
must then list the pipeline and the plugin and run both.

A dry run against a local build of the same version sets VERIFY_UPGRADE_SAME_VERSION=1; the installed bytes must still
be NEW's and differ from OLD's.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time

APP_ID = "arcade.box"
FILENAME = "Arcade-Box.AppImage"
PLUGIN = Path(__file__).resolve().parents[1] / "crates/arcade-plugin-host/tests/fixtures/community-uppercase"

# The settings table of a user who changed what Settings offers (`save_folder` is filled in with a real folder).
SETTINGS = {"theme": "dark", "onboarding_complete": "true", "global_shortcut": "Ctrl+Alt+K",
            "link_enabled": "true", "link_disabled_peers": '["arcade.look"]'}
FAVORITES = ["arcade.text.case", "arcade.text.diff", "arcade.convert.calculator"]
# Recent tools are the history rows (newest id first) and the usage counts behind the ranking.
USAGE = {"arcade.text.case": (12, "2026-05-01 09:30:00"), "arcade.text.lorem": (3, "2026-05-02 18:45:10"),
         "arcade.convert.calculator": (7, "2026-05-03 08:00:00")}
HISTORY = [(101, "arcade.text.case", "success", "2026-05-01 09:30:00"),
           (102, "arcade.text.lorem", "error", "2026-05-02 18:45:10"),
           (103, "arcade.convert.calculator", "success", "2026-05-03 08:00:00")]
ALIASES = {"shout": "arcade.text.case", "lipsum": "arcade.text.lorem"}
PROVIDER_CHOICES = {"image.vips": "/usr/bin/vips"}
JOB = ("job-seed-1", "arcade.text.case", "succeeded", 1.0, "2026-05-01 09:30:00", "2026-05-01 09:30:01")
PIPELINE = {"id": "tidy-shout", "name": "Tidy and shout", "version": 2,
            "nodes": [{"id": "snake", "toolId": "arcade.text.case", "inputs": [{"kind": "external", "index": 0}],
                       "options": {"mode": "snake"}},
                      {"id": "upper", "toolId": "arcade.text.case",
                       "inputs": [{"kind": "node", "nodeId": "snake", "outputIndex": 0}],
                       "options": {"mode": "upper"}}],
            "outputNodes": ["upper"]}


def clipboard_history():
    """`clipboard-history.json` as Box 0.1.0 through 0.4.3 wrote it: no `schema` field. One pinned item far past the
    retention, one recent text and one recent image (2 x 2 RGBA)."""
    now = int(time.time())
    return {"enabled": True, "retentionDays": 30, "excludedApplications": ["keepass"], "items": [
        {"id": "1700000000000-1", "kind": "text", "text": "hello world", "imageRgbaBase64": None, "imageWidth": None,
         "imageHeight": None, "preview": "hello world", "capturedAt": 1700000000, "pinned": True,
         "sourceApplication": "Terminal"},
        {"id": f"{now - 600}000-2", "kind": "text", "text": "second item", "imageRgbaBase64": None,
         "imageWidth": None, "imageHeight": None, "preview": "second item", "capturedAt": now - 600, "pinned": False,
         "sourceApplication": None},
        {"id": f"{now - 300}000-3", "kind": "image", "text": None, "imageRgbaBase64": "AAAAAP8AAAD/AAAA/w==",
         "imageWidth": 2, "imageHeight": 2, "preview": "Image 2x2", "capturedAt": now - 300, "pinned": False,
         "sourceApplication": None}]}


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def alive(pid):
    """Running, not a zombie (OLD's launcher is this script's own child until it is reaped)."""
    try:
        return Path(f"/proc/{pid}/stat").read_text().rpartition(") ")[2].split()[0] != "Z"
    except OSError:
        return False


def box_processes(root):
    """Processes running a file of an unpacked copy of Box under `root` (Link U1): {pid: executable}."""
    found = {}
    for proc in Path("/proc").iterdir():
        try:
            exe = os.readlink(proc / "exe")
        except OSError:
            continue
        if exe.startswith(str(root) + "/") and "/arcade/runtime/box-" in exe and alive(proc.name):
            found[int(proc.name)] = exe
    return found


def descendants(pid):
    """`pid` and every process below it."""
    parents = {}
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit():
            try:
                parents[int(entry.name)] = int((entry / "stat").read_text().rpartition(") ")[2].split()[1])
            except (OSError, IndexError, ValueError):
                pass
    family = {pid}
    while True:
        children = {child for child, parent in parents.items() if parent in family}
        if children <= family:
            return family
        family |= children


def subset(expected, actual, where):
    """Every key, element and value of `expected` is in `actual` (a newer Box may add fields). Lists of objects with
    an `id` match by it, lists of values must be contained, other lists compare element by element."""
    if isinstance(expected, dict):
        assert isinstance(actual, dict), f"{where}: {actual!r} is not an object"
        for key, value in expected.items():
            assert key in actual, f"{where}.{key} is missing: {actual}"
            subset(value, actual[key], f"{where}.{key}")
    elif isinstance(expected, list):
        assert isinstance(actual, list), f"{where}: {actual!r} is not a list"
        if all(isinstance(item, dict) and "id" in item for item in expected):
            by_id = {item.get("id"): item for item in actual if isinstance(item, dict)}
            for item in expected:
                assert item["id"] in by_id, f"{where}: {item['id']!r} is missing from {sorted(map(str, by_id))}"
                subset(item, by_id[item["id"]], f"{where}[{item['id']}]")
        elif all(not isinstance(item, (dict, list)) for item in expected):
            missing = [item for item in expected if item not in actual]
            assert not missing, f"{where}: {missing} missing from {actual}"
        else:
            assert len(expected) <= len(actual), f"{where}: {actual!r} is shorter than {expected!r}"
            for index, item in enumerate(expected):
                subset(item, actual[index], f"{where}[{index}]")
    else:
        assert expected == actual, f"{where}: {actual!r}, expected {expected!r}"


def seed_database(path, save_folder):
    """Fills the database OLD created (its own schema, migrations 1 and 2) the way using OLD fills it."""
    settings = dict(SETTINGS, save_folder=str(save_folder))
    connection = sqlite3.connect(path)
    try:
        with connection:
            connection.executemany("INSERT OR REPLACE INTO favorites (tool_id) VALUES (?)", [(t,) for t in FAVORITES])
            connection.executemany("INSERT OR REPLACE INTO usage (tool_id, uses, last_used) VALUES (?, ?, ?)",
                                   [(tool, *value) for tool, value in USAGE.items()])
            connection.executemany("INSERT OR REPLACE INTO history (id, tool_id, status, created_at) "
                                   "VALUES (?, ?, ?, ?)", HISTORY)
            connection.executemany("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)", settings.items())
            connection.executemany("INSERT OR REPLACE INTO custom_aliases (alias, tool_id) VALUES (?, ?)",
                                   ALIASES.items())
            connection.executemany("INSERT OR REPLACE INTO provider_choices (capability, executable_path) "
                                   "VALUES (?, ?)", PROVIDER_CHOICES.items())
            connection.execute("INSERT OR REPLACE INTO jobs (id, tool_id, status, progress, created_at, updated_at) "
                               "VALUES (?, ?, ?, ?, ?, ?)", JOB)
    finally:
        connection.close()


def read_database(path):
    connection = sqlite3.connect(path)
    try:
        def rows(query):
            return connection.execute(query).fetchall()
        return {"migrations": {row[0] for row in rows("SELECT version FROM schema_migrations")},
                "favorites": {row[0] for row in rows("SELECT tool_id FROM favorites")},
                "usage": {row[0]: row[1:] for row in rows("SELECT tool_id, uses, last_used FROM usage")},
                "history": {row[0]: row for row in rows("SELECT id, tool_id, status, created_at FROM history")},
                "settings": dict(rows("SELECT key, value FROM settings")),
                "aliases": dict(rows("SELECT alias, tool_id FROM custom_aliases")),
                "providers": dict(rows("SELECT capability, executable_path FROM provider_choices")),
                "jobs": {row[0]: row for row in rows(
                    "SELECT id, tool_id, status, progress, created_at, updated_at FROM jobs")},
                "pipelines": {row[0]: (row[1], json.loads(row[2]))
                              for row in rows("SELECT id, version, definition_json FROM pipelines")}}
    finally:
        connection.close()


def check_database(path, save_folder, label):
    """Every row seeded is still there with its value; rows a newer Box adds are allowed."""
    data = read_database(path)
    assert {1, 2} <= data["migrations"], f"{label}: schema_migrations lost a version: {data['migrations']}"
    assert set(FAVORITES) <= data["favorites"], f"{label}: favorites {sorted(data['favorites'])}"
    for tool, value in USAGE.items():
        assert data["usage"].get(tool) == value, f"{label}: usage of {tool}: {data['usage'].get(tool)}, expected {value}"
    for row in HISTORY:
        assert data["history"].get(row[0]) == row, f"{label}: history {row[0]}: {data['history'].get(row[0])}"
    for key, value in dict(SETTINGS, save_folder=str(save_folder)).items():
        assert key in data["settings"], f"{label}: setting {key} is missing: {data['settings']}"
        actual = data["settings"][key]
        if key == "link_disabled_peers":
            actual, value = json.loads(actual), json.loads(value)
        assert actual == value, f"{label}: setting {key} is {actual!r}, expected {value!r}"
    subset(ALIASES, data["aliases"], f"{label} aliases")
    subset(PROVIDER_CHOICES, data["providers"], f"{label} provider choices")
    assert data["jobs"].get(JOB[0]) == JOB, f"{label}: job {JOB[0]}: {data['jobs'].get(JOB[0])}"
    version, definition = data["pipelines"].get(PIPELINE["id"], (None, None))
    assert version == PIPELINE["version"], f"{label}: pipeline {PIPELINE['id']} is at version {version}"
    subset(PIPELINE, definition, f"{label} pipeline")


def plugin_files(data):
    """Every file of the installed plugins: {relative path: sha256}."""
    root = data / "plugins"
    return {str(path.relative_to(root)): digest(path) for path in sorted(root.rglob("*")) if path.is_file()}


def main():
    old_image, new_image = (Path(arg).resolve(strict=True) for arg in sys.argv[1:3])
    assert PLUGIN.joinpath("plugin.json").is_file(), f"The example plugin is missing: {PLUGIN}"
    assert digest(old_image) != digest(new_image), "OLD and NEW are the same file"
    # Link treats executables anywhere under a .git tree as development builds.
    with tempfile.TemporaryDirectory(prefix="box-upgrade-", dir=os.environ.get("RUNNER_TEMP")) as directory:
        root = Path(directory).resolve()
        for parent in (root, *root.parents):
            if (parent / ".git").exists():
                raise RuntimeError("Upgrade verification must run outside the checkout")
        profile = root / "profile"
        env = {k: v for k, v in os.environ.items() if not k.startswith("ARCADE_") and k not in (
            "APPIMAGE", "APPDIR", "LD_LIBRARY_PATH", "WAYLAND_DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE", "SWAYSOCK")}
        env.update(ARCADE_HOME=str(profile), HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"),
                   XDG_DATA_HOME=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"),
                   XDG_RUNTIME_DIR=str(root / "run"), ARCADE_NO_INSTALL_PROMPT="1", GDK_BACKEND="x11",
                   APPIMAGE_EXTRACT_AND_RUN="1")
        for name in ("home", "config", "data", "cache", "run"):
            (root / name).mkdir(mode=0o700)
        downloads = root / "home/Downloads"
        downloads.mkdir()
        data = profile / ".local/share/arcade/box"
        endpoint = profile / "run" / (APP_ID + ".endpoint")
        installed = profile / "Applications/Arcade" / FILENAME
        log = (root / "box.log").open("w+b")
        launched = []

        def download(image):
            copy = downloads / image.name
            shutil.copy2(image, copy)
            copy.chmod(0o755)
            return copy

        def run(executable, *args):
            result = subprocess.run([str(executable), *args], env=env, capture_output=True, text=True, timeout=120)
            assert result.returncode == 0, (
                f"{Path(executable).name} {' '.join(args)} exited {result.returncode}: {result.stderr[-2000:]}")
            return result.stdout

        def resident():
            try:
                pid = json.loads(endpoint.read_text())["pid"]
            except (OSError, ValueError, KeyError):
                return None
            return pid if alive(pid) else None

        def wait(probe, what, seconds=30):
            end = time.monotonic() + seconds
            while time.monotonic() < end:
                if result := probe():
                    return result
                time.sleep(0.1)
            raise AssertionError(f"Timed out waiting for {what}")

        def start():
            launched.append(subprocess.Popen([str(installed), "--background"], env=env, stdout=log,
                                             stderr=subprocess.STDOUT, start_new_session=True))
            return wait(resident, "Box to start")

        def stop():
            pid = resident()
            subprocess.run([str(installed), "--quit"], env=env, capture_output=True, timeout=30)
            if pid:
                wait(lambda: not alive(pid) and not resident(), "--quit to stop the resident")
            wait(lambda: not box_processes(root) and all(p.poll() is not None for p in launched),
                 "every Box process to exit after --quit", 15)

        try:
            # OLD is installed from a download; a fresh install starts at login (Box 0.4.3 has no flag for it).
            old = json.loads(run(download(old_image), "--install", "--silent", "--no-launch"))
            assert installed == Path(old["path"]), old
            assert digest(installed) == digest(old_image), "The installed copy is not OLD's bytes"
            old_login = Path(old["integration"]["autostart"])
            assert old_login.is_file() and "Hidden=true" not in old_login.read_text(), "OLD does not start at login"
            old_version = run(installed, "--version").strip().split()[-1]
            assert old_version == old["version"], (old_version, old)
            terminal = Path(old["integration"]["commands"][0])
            assert terminal.name == "arcade-box" and terminal.resolve() == installed, old["integration"]

            # The user's data, in the shapes OLD writes. OLD's own commands save the pipeline and install the plugin
            # (and create the database with its own schema); the rest goes into the same tables and file.
            definition = root / "pipeline.json"
            definition.write_text(json.dumps(PIPELINE))
            run(terminal, "pipeline", "save", definition)
            run(terminal, "plugins", "install", PLUGIN)
            save_folder = root / "home/Saved"
            save_folder.mkdir()
            seed_database(data / "arcade.sqlite3", save_folder)
            clipboard = clipboard_history()
            (data / "clipboard-history.json").write_text(json.dumps(clipboard, indent=2))
            plugins_before = plugin_files(data)
            assert any(name.endswith("installed.json") for name in plugins_before), plugins_before

            # OLD reads it all and rewrites what it owns, then stops.
            start()
            time.sleep(5)
            stop()
            check_database(data / "arcade.sqlite3", save_folder, f"{old_version} after it ran")
            subset(clipboard, json.loads((data / "clipboard-history.json").read_text()), f"{old_version} clipboard")
            assert plugin_files(data) == plugins_before, "OLD changed the installed plugin"

            # OLD runs again; the update is by hand while it does.
            old_resident = start()
            time.sleep(3)
            # OLD (0.4.3) predates the unpacked copy (Link U1): its processes are the launcher and what it started.
            old_pids = {old_resident, *descendants(launched[-1].pid)}
            print(f"{old_version} installed at {installed} and running (PID {old_resident}) with the user's data",
                  flush=True)
            # What a double click does: install over it, stop the running copy, start the installed one.
            with (root / "update.log").open("w+b") as update:
                result = subprocess.run([str(download(new_image)), "--install", "--silent"], env=env, stdout=update,
                                        stderr=subprocess.STDOUT, timeout=120)
                update.seek(0)
                output = update.read().decode(errors="replace")
            assert result.returncode == 0, f"{new_image.name} --install exited {result.returncode}: {output[-2000:]}"
            new_resident = wait(lambda: (pid if (pid := resident()) and pid not in old_pids else None),
                                "NEW to start after the install")
            version = run(installed, "--version").strip().split()[-1]
            # The candidate on main has OLD's version number until the release is cut.
            assert version != old_version or os.environ.get("VERIFY_UPGRADE_SAME_VERSION") == "1", (
                f"NEW is {version}, the same release as OLD")
            assert digest(installed) == digest(new_image), "The installed copy is not NEW's bytes"
            wait(lambda: not any(alive(pid) for pid in old_pids), "OLD to exit", 15)
            assert new_resident in box_processes(root), f"NEW's PID {new_resident} is not from an unpacked copy"
            status = json.loads(run(installed, "--status"))
            assert status["running"] and status["runningVersion"] == version and status["app"] == APP_ID, status

            # The integration is NEW's: one launcher, the commands and the login entry.
            integration = json.loads(run(installed, "--integration-status"))
            receipt = integration["receipt"]
            assert receipt["version"] == version and Path(receipt["path"]) == installed, receipt
            missing = [entry["path"] for entry in integration["entries"] if not entry["exists"]]
            assert not missing, f"Integration entries missing after the update: {missing}"
            entries = receipt["integration"]
            login = Path(entries["autostart"])
            assert login == old_login, f"The login entry moved: {old_login} -> {login}"
            text = login.read_text() if login.is_file() else ""
            assert text and "Hidden=true" not in text, "Start at login was not kept"
            assert str(installed) in text and "--background" in text, text
            assert not [path for path in login.parent.glob("*.desktop")
                        if path != login and "Arcade Box" in path.read_text()], "A second login entry exists"
            assert Path(entries["cli"]).resolve() == installed, f"{entries['cli']} -> {Path(entries['cli']).resolve()}"
            assert [Path(path).resolve() for path in entries["commands"]] == [installed], entries["commands"]
            assert terminal.resolve() == installed, f"{terminal} -> {terminal.resolve()}"
            desktop = Path(entries["desktopEntry"])
            assert desktop == Path(old["integration"]["desktopEntry"]), "The launcher moved"
            assert desktop.is_file() and str(installed) in desktop.read_text(), desktop
            launchers = [path for path in desktop.parent.glob("*.desktop") if f"X-Arcade-Id={APP_ID}" in path.read_text()]
            assert launchers == [desktop], f"More than one Box launcher is listed: {launchers}"
            # The installer keeps the replaced copy in `.previous`; no other AppImage sits beside the installed one.
            beside = sorted(path.name for path in installed.parent.iterdir() if path.name.endswith(".AppImage"))
            assert beside == [FILENAME], f"Other packages beside the installed copy: {beside}"
            assert [path.name for path in (profile / "installs").glob("*.json")] == [APP_ID + ".json"]

            stop()
            assert not resident(), "NEW still runs after --quit"
            for name in ("box.log", "update.log"):
                assert b"panicked at" not in (root / name).read_bytes(), f"Box logged a panic ({name})"

            # The data: every value seeded, read from the files after NEW ran and stopped.
            check_database(data / "arcade.sqlite3", save_folder, f"{version} after the update")
            subset(clipboard, json.loads((data / "clipboard-history.json").read_text()), f"{version} clipboard")
            after = plugin_files(data)
            state = json.loads((data / "plugins/arcade.community.uppercase/installed.json").read_text())
            assert state["activeVersion"] == "0.1.0" and state["grants"] == [], state
            assert {k: v for k, v in after.items() if not k.endswith("installed.json")} == \
                {k: v for k, v in plugins_before.items() if not k.endswith("installed.json")}, "NEW changed the plugin"
            # NEW's own command reads and runs the saved pipeline and the installed plugin.
            assert "tidy-shout" in run(terminal, "pipeline", "list"), "NEW does not list the saved pipeline"
            assert run(terminal, "pipeline", "run", "tidy-shout", "Hello World").strip() == "HELLO_WORLD"
            assert "arcade.community.uppercase" in run(terminal, "plugins", "list"), "NEW does not list the plugin"
            assert run(terminal, "run", "arcade.community.uppercase", "upgraded").strip() == "UPGRADED"
            print(f"{version} replaced {old_version} in place (PID {new_resident}): settings, favorites, history, "
                  f"aliases, jobs, pipeline, plugin, Link choices and clipboard history kept, Start at login kept; "
                  f"launcher, commands and login entry are NEW's", flush=True)
        except Exception:
            log.seek(0)
            print(log.read().decode(errors="replace")[-6000:], file=sys.stderr)
            raise
        finally:
            if resident():
                subprocess.run([str(installed), "--quit"], env=env, capture_output=True, timeout=30)
            for pid in {*box_processes(root), resident()} - {None}:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            for process in launched:
                if process.poll() is None:
                    process.kill()
                process.wait(timeout=15)
    print(f"PASS: {old_version} -> {version} keeps the user's data and integration")


if __name__ == "__main__":
    main()
