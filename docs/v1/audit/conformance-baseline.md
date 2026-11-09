# Conformance baseline (2026-11-07)

`arcade-link conformance <AppImage>` (Link branch `claude/tender-feynman-3vt7dg`, `specs/LINK.md` §5.2) run against the
eight currently released AppImages, downloaded from each repo's latest GitHub release, in an isolated HOME with no
display (`APPIMAGE_EXTRACT_AND_RUN=1`, `QT_QPA_PLATFORM=offscreen`). Repository-only checks (REPO-2, REL-1, REL-2,
LINK-2) and the desktop-harness checks (RT-*, UI-*, PERF-*) report SKIP here and are left out below. This is expected
to fail today: it is the to-do list for the app specs.

| App (release) | Failing checks |
|---|---|
| Tools 0.4.6 | ID-1, ID-2, ID-3 (desktop file is `Arcade Tools.desktop`), REPO-1 (no LICENSE, no notices), CLI-1 and CLI-2 (panics, exit 101, without a display) |
| Box 0.4.3 | ID-1, ID-2, ID-3 (`Arcade Box.desktop`), REPO-1, CLI-2 (unknown flag opens the install dialog, exit 101) |
| Clipboard 0.3.2 | REPO-1 (no notices), CLI-1, CLI-2, CLI-3, CLI-4 (every command opens the install dialog first) |
| Look 0.4.3 | ID-1, ID-2, ID-3 (`Arcade Look.desktop`), REPO-1, CLI-2 (install dialog on an unknown flag) |
| Find 0.5.4 | CLI-2 (unknown flag opens the install dialog, exit 1) |
| Lens 0.4.2 | REPO-1, CLI-1 and CLI-2 (`--status` creates a winit event loop and fails without a display) |
| Shelf 0.4.0 | CLI-2 (unknown flag exits 1, not 2) |
| Wheel 0.5.0 | REPO-1 (no notices), CLI-1 (exit 1), CLI-2 (an unknown flag starts the app and doesn't exit) |

## Full output

### Arcade-Box_0.4.3_amd64.AppImage

```
FAIL  ID-1    no dev.arcade.<Name>.desktop entry found (found ["Arcade Box.desktop", "Arcade Box.desktop"])
FAIL  ID-2    0 launcher ids, 0 autostart entries
FAIL  ID-3    app icon dev.arcade.<Name>.svg/png: false, symbolic tray icon: true
FAIL  REPO-1  missing: LICENSE, THIRD_PARTY_NOTICES
PASS  CLI-1   --status prints one line of JSON
FAIL  CLI-2   unknown flag: exit 101, stderr "arcade-desktop: install dialog: no display for the install dialog: Failed to initialize GTK"; --help: exit 0
PASS  CLI-3   --version: Arcade Box 0.4.3
PASS  CLI-4   manifest for arcade.box 0.4.3
PASS  FS-1    files only under arcade/ folders
24 checks, 5 failed
```

### Arcade-Clipboard-0.3.2-x86_64.AppImage

```
PASS  ID-1    desktop id dev.arcade.Clipboard
PASS  ID-2    one launcher entry, at most one autostart entry
PASS  ID-3    app icon and symbolic tray icon
FAIL  REPO-1  missing: THIRD_PARTY_NOTICES
FAIL  CLI-1   exit 1, 0 stdout lines, stderr: arcade-clipboard: install dialog: no display for the install dialog: Failed to initialize GTK
FAIL  CLI-2   unknown flag: exit 1, stderr "arcade-clipboard: install dialog: no display for the install dialog: Failed to initialize GTK"; --help: exit 0
FAIL  CLI-3   exit 1, output ""
FAIL  CLI-4   exit 1: EOF while parsing a value at line 1 column 0; stderr: 
PASS  FS-1    files only under arcade/ folders
24 checks, 5 failed
```

### Arcade-Find-x86_64.AppImage

```
PASS  ID-1    desktop id dev.arcade.Find
PASS  ID-2    one launcher entry, at most one autostart entry
PASS  ID-3    app icon and symbolic tray icon
PASS  REPO-1  licence, notices, security policy and changelog present
PASS  CLI-1   --status prints one line of JSON
FAIL  CLI-2   unknown flag: exit 1, stderr "arcade-find: install dialog: winit EventLoopError: os error at /home/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-0.30.13/src/platform_impl"; --help: exit 0
PASS  CLI-3   --version: arcade-find 0.5.4
PASS  CLI-4   manifest for arcade.find 0.5.4
PASS  FS-1    files only under arcade/ folders
24 checks, 1 failed
```

### Arcade-Look_0.4.3_amd64.AppImage

```
FAIL  ID-1    no dev.arcade.<Name>.desktop entry found (found ["Arcade Look.desktop", "Arcade Look.desktop"])
FAIL  ID-2    0 launcher ids, 0 autostart entries
FAIL  ID-3    app icon dev.arcade.<Name>.svg/png: false, symbolic tray icon: true
FAIL  REPO-1  missing: LICENSE, THIRD_PARTY_NOTICES
PASS  CLI-1   --status prints one line of JSON
FAIL  CLI-2   unknown flag: exit 101, stderr "arcade-look: install dialog: no display for the install dialog: Failed to initialize GTK"; --help: exit 0
PASS  CLI-3   --version: arcade-look 0.4.3
PASS  CLI-4   manifest for arcade.look 0.4.3
PASS  FS-1    files only under arcade/ folders
24 checks, 5 failed
```

### Arcade-Shelf-0.4.0-x86_64.AppImage

```
PASS  ID-1    desktop id dev.arcade.Shelf
PASS  ID-2    one launcher entry, at most one autostart entry
PASS  ID-3    app icon and symbolic tray icon
PASS  REPO-1  licence, notices, security policy and changelog present
PASS  CLI-1   --status prints one line of JSON
FAIL  CLI-2   unknown flag: exit 1, stderr "Detected locale \"C\" with character encoding \"ANSI_X3.4-1968\", which is not UTF-8."; --help: exit 0
PASS  CLI-3   --version: Arcade Shelf 0.4.0
PASS  CLI-4   manifest for arcade.shelf 0.4.0
PASS  FS-1    files only under arcade/ folders
24 checks, 1 failed
```

### Arcade-Tools_0.4.6_amd64.AppImage

```
FAIL  ID-1    no dev.arcade.<Name>.desktop entry found (found ["Arcade Tools.desktop", "Arcade Tools.desktop"])
FAIL  ID-2    0 launcher ids, 0 autostart entries
FAIL  ID-3    app icon dev.arcade.<Name>.svg/png: false, symbolic tray icon: true
FAIL  REPO-1  missing: LICENSE, THIRD_PARTY_NOTICES
FAIL  CLI-1   exit 101, 0 stdout lines, stderr: 
FAIL  CLI-2   unknown flag: exit 101, stderr ""; --help: exit 0
PASS  CLI-3   --version: Arcade Tools 0.4.6
PASS  CLI-4   manifest for arcade.tools 0.4.6
PASS  FS-1    files only under arcade/ folders
24 checks, 6 failed
```

### ArcadeLens-0.4.2-linux-x86_64.AppImage

```
PASS  ID-1    desktop id dev.arcade.Lens
PASS  ID-2    one launcher entry, at most one autostart entry
PASS  ID-3    app icon and symbolic tray icon
FAIL  REPO-1  missing: LICENSE, THIRD_PARTY_NOTICES
FAIL  CLI-1   exit 1, 0 stdout lines, stderr: arcade-lens: winit EventLoopError: os error at /home/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-0.30.13/src/platform_impl/linux/mod.rs:76
FAIL  CLI-2   unknown flag: exit 1, stderr "arcade-lens: winit EventLoopError: os error at /home/runner/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-0.30.13/src/platform_impl/linux/mod.rs:76"; --help: exit 0
PASS  CLI-3   --version: Arcade Lens 0.4.2
PASS  CLI-4   manifest for arcade.lens 0.4.2
PASS  FS-1    files only under arcade/ folders
24 checks, 3 failed
```

### ArcadeWheel-0.5.0-Linux-x86_64.AppImage

```
PASS  ID-1    desktop id dev.arcade.Wheel
PASS  ID-2    one launcher entry, at most one autostart entry
PASS  ID-3    app icon and symbolic tray icon
FAIL  REPO-1  missing: THIRD_PARTY_NOTICES
FAIL  CLI-1   exit 1, 1 stdout lines, stderr: Detected locale "C" with character encoding "ANSI_X3.4-1968", which is not UTF-8.
FAIL  CLI-2   /tmp/claude-0/-home-user/f00dc568-6146-58d5-98aa-7bea9ca82c16/scratchpad/released/ArcadeWheel-0.5.0-Linux-x86_64.AppImage --arcade-conformance-unknown-flag did not exit within 20 s
PASS  CLI-3   --version: Arcade Wheel 0.5.0
PASS  CLI-4   manifest for arcade.wheel 0.5.0
PASS  FS-1    files only under arcade/ folders
24 checks, 3 failed
```

