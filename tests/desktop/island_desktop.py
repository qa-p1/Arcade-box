#!/usr/bin/env python3
"""Tier 1 (headless Hyprland, Link's desktop harness) for Arcade Box's Island.

    island_desktop.py <AppImage>

In a private profile: the Island's identity (ARCADE_APP.md ID-1), the shortcut opening and closing it with a press and
release, the Island staying open when the pointer moves into another window (fleet popup rule), and the audit's
compositor findings (docs/v1/audit/box-real.md): BR-1 a lost release wedging the shortcut, BR-2 the invisible part of
the Island taking clicks, BR-0 Escape in a tool closing the Island. A check named "KNOWN BUG <id>" must find the bug
(XFAIL) until the fix lands, then it reports XPASS and fails the run until the marker is removed (docs/v1/DECISIONS.md
D3). Exit 0 when every check is as expected, 1 otherwise.
"""
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, ".arcade-link/tools/desktop")
from arcade_desktop import Desktop  # noqa: E402

CLASS, TITLE, SHORTCUT = "dev.arcade.Box", "Arcade Box", "arcade-box-toggle"
d = Desktop()
home = tempfile.mkdtemp(prefix="box-desktop-")
downloads = Path(home, "Downloads")
downloads.mkdir()
app = str(downloads / "Arcade-Box.AppImage")
shutil.copy2(sys.argv[1], app)
os.chmod(app, 0o755)
env = {**os.environ, "HOME": home, "XDG_CONFIG_HOME": f"{home}/.config", "XDG_DATA_HOME": f"{home}/.local/share",
       "XDG_CACHE_HOME": f"{home}/.cache", "ARCADE_HOME": f"{home}/arcade-link"}
log = open(d.artifact("box.log"), "wb")
results = []


def run(*args, timeout=60):
    return subprocess.run([app, *args], env=env, stdout=log, stderr=subprocess.STDOUT, timeout=timeout)


def fail(message):
    d.screenshot("failed")
    print(f"FAILED: {message}")
    sys.exit(1)


def check(name, ok, detail=""):
    """Record one check; a KNOWN BUG check passes when it fails."""
    if name.startswith("KNOWN BUG"):
        status = "XFAIL" if not ok else "XPASS"
    else:
        status = "PASS" if ok else "FAIL"
    if status in ("FAIL", "XFAIL"):
        d.screenshot(name.split(":")[0].replace(" ", "-").lower())
    results.append((status, name, detail))
    print(f"{status} {name}{': ' + detail if detail else ''}", flush=True)


def island():
    return d.find_window(CLASS, TITLE)


def island_shown(timeout=5):
    try:
        return d.wait(lambda: (w := island()) and w.get("mapped", True) and not w.get("hidden") and w, timeout, "Island shown")
    except AssertionError:
        return None


def island_hidden(timeout=3):
    try:
        d.wait(lambda: not ((w := island()) and w.get("mapped", True) and not w.get("hidden")), timeout, "Island hidden")
        return True
    except AssertionError:
        return False


def type_text(address, text):
    for char in text:
        d.send_shortcut(address, "", "space" if char == " " else char)
        time.sleep(0.03)


def press(hold=0.08):
    d.global_shortcut(SHORTCUT, hold=hold)


installed = run("--install", "--silent", "--no-launch", timeout=120)
if installed.returncode != 0:
    fail(f"--install --silent exited {installed.returncode}")
copies = [p for p in Path(home).rglob("Arcade-Box*.AppImage") if downloads not in p.parents]
if len(copies) != 1:
    fail(f"installed copies: {copies}")
app = str(copies[0])

# The first-run welcome replaces the search until it is finished; finish it in the profile's own database, as the
# "Skip" button would, so the checks below start from the search.
resident = subprocess.Popen([app, "--background"], env=env, stdout=log, stderr=subprocess.STDOUT)
try:
    d.wait(lambda: SHORTCUT in d.bound_shortcuts(), 30, "the Island shortcut to be bound")
except AssertionError:
    fail(f"the shortcut was never bound: {d.bound_shortcuts()}")
run("--quit")
resident.wait(timeout=30)
databases = [p for p in Path(home, ".local/share/arcade/box").rglob("*") if p.suffix in (".db", ".sqlite", ".sqlite3")]
if not databases:
    fail("no Box database after the first start")
with sqlite3.connect(databases[0]) as db:
    db.execute("INSERT OR REPLACE INTO settings (key, value) VALUES ('onboarding_complete', 'true')")

other = subprocess.Popen(["foot", "--app-id", "box-test-other", "--title", "Other window", "sh", "-c", "sleep 600"],
                         stdout=log, stderr=subprocess.STDOUT)
d.wait_window("box-test-other", timeout=15)
resident = subprocess.Popen([app, "--background"], env=env, stdout=log, stderr=subprocess.STDOUT)
d.wait(lambda: SHORTCUT in d.bound_shortcuts(), 30, "the Island shortcut to be bound again")

try:
    # The shortcut opens the Island and, pressed again, closes it.
    press()
    shown = island_shown()
    check("The shortcut opens the Island", bool(shown))
    if not shown:
        fail("the Island never opened; nothing else can be checked")
    check("The Island is dev.arcade.Box, titled Arcade Box (ID-1)", shown.get("class") == CLASS, shown.get("class", ""))
    check("The Island floats", bool(shown.get("floating")))

    # Fleet popup rule: moving the pointer into another window does not close it.
    other_window = d.find_window("box-test-other")
    d.pointer_move(*d.center_of(other_window))
    time.sleep(1.5)
    check("The Island stays open when the pointer moves into another window", bool(island_shown(1)))

    # BR-2: the part of the 740×740 window below the search card is invisible and must let clicks through.
    shown = island()
    x, y = shown["at"]
    width, height = shown["size"]
    d.pointer_move(x + width // 2, y + height - 60)
    d.click()
    time.sleep(0.5)
    active = d.hyprctl("-j", "activewindow")
    check("KNOWN BUG BR-2: a click below the search card reaches the window underneath", "box-test-other" in active,
          "the Island took it" if "box-test-other" not in active else "")

    press()
    check("The shortcut closes the Island again", island_hidden())

    # BR-1: a press whose release never arrives must not wedge the shortcut.
    portal = d._portal()
    portal.Press(SHORTCUT)
    lost = island_shown()
    if lost:
        d.escape(lost["address"])
        island_hidden()
    time.sleep(1.5)
    press()
    reopened = island_shown(3)
    check("KNOWN BUG BR-1: the shortcut still works after a lost release", bool(reopened))
    portal.Release(SHORTCUT)
    if not reopened:
        press()
        reopened = island_shown()
    if not reopened:
        fail("the Island would not open for the Escape checks")

    # BR-0: Escape in a tool goes back to the search; it never closes the Island.
    address = reopened["address"]
    d.focus(address)
    type_text(address, "calculator")
    time.sleep(0.8)
    d.send_shortcut(address, "", "Return")
    time.sleep(0.8)
    d.escape(address)
    time.sleep(0.8)
    check("KNOWN BUG BR-0: Escape in a tool keeps the Island open", bool(island_shown(1)))
    if island_shown(1):
        d.escape(address)  # clears the search
        time.sleep(0.4)
        d.escape(address)  # closes the empty search
        check("Escape in the empty search closes the Island", island_hidden())

    quit_status = run("--quit")
    check("--quit stops Box", quit_status.returncode == 0 and resident.wait(timeout=30) is not None)
finally:
    other.kill()
    if resident.poll() is None:
        resident.kill()

bad = [r for r in results if r[0] in ("FAIL", "XPASS")]
print(f"{len(results) - len(bad)} of {len(results)} checks as expected")
sys.exit(1 if bad else 0)
