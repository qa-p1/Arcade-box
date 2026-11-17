# Changelog

All notable changes to Arcade Box. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-11-15

### Added

- Settings → About opens the third-party notices, which every package now includes.
- The calculator has an Angles setting for degrees; angles are radians by default.
- Audio and video previews play from the AppImage (WebM, Ogg, MP3, WAV, FLAC, Matroska and the MP4 container). H.264 and AAC media still does not preview.

### Changed

- Clipboard history files carry a schema version. An unreadable history file is kept aside and Box starts with an empty history instead of failing to start; fields written by a newer Box are kept.
- Uninstalling Box also removes its unpacked runtime copies.
- Switches and checkboxes show the focus ring when reached with Tab, and the narrow sidebar's category buttons have names for screen readers.
- The calculator reads `10 % 3` as a remainder, `50%` as a percentage and `100 + 10%` as 110. `!` is a factorial unless it starts `!=`.
- Calculator results round to 15 significant digits, so `sin(pi)` is 0 and `0.1 + 0.2` is 0.3.
- Converters read `1,000` and `12,345,678` with thousands separators and `1,5 m` as 1.5 m. Inside a calculator function a comma separates arguments.
- Time zone abbreviations such as `EST`, `PST` and `IST` stand for their region's zone, summer offsets included, and the label shows both (`America/New_York (EST)`). IANA names match in any case.
- Times without a date (`9:00`, `5pm`, `tomorrow 5pm`, `today at 14:00`) mean today in the source zone. Plain digits are Unix time, with the unit picked by the digit count.
- The Island opens a fresh search each time it is shown. A job started from a tool you left keeps running and stays reachable from the search's Jobs indicator.
- Sizes read the same as in the other Arcade apps (for example "1.2 MB").
- Box uses less memory while it runs in the background: a few seconds after the last window closes it ends WebKit's network process and pages out idle memory, and while a window is open it trims itself periodically.
- A background start from the AppImage runs from an unpacked copy under `<data>/arcade/runtime/`, so no FUSE helper stays running.
- Clipboard history keeps at most 64 MiB of text and image data, dropping the oldest unpinned items first.
- The plugin sandbox runs on Wasmtime 36 LTS, which has the fixes for the security advisories found in the previous version. The zip, calamine and XLSX export libraries are updated for the same reason.
- Releases start as release candidates on a hidden test channel that Arcade Tools never offers, and are promoted to stable only after every check passes. A signed prerelease is published each night from `main`.
- Promotion to stable refuses to run without the update signing key, so nothing is published unsigned.
- Box moves to Arcade Link 1.0.11 and its shared design kit: the font stack gains the colour-emoji fonts, and a tab panel keeps its focus outline for keyboard focus.

### Fixed

- Linux: the AppImage starts on Ubuntu 22.04 and other systems of that age. It was built on Ubuntu 24.04 and needed its newer C++ library.
- Copy in a pinned image's window works: its content security policy blocked reading the image it copies.
- The Island and dashboard run under a strict content security policy, like the pinned-image window: no remote scripts, images or connections.
- The global shortcut works on desktops with xdg-desktop-portal older than 1.19 (Ubuntu 24.04), which have no host app registry.
- On Hyprland, clicks outside the Island's card reach the window underneath.
- After Restart, Box runs from its unpacked copy again instead of from the AppImage's FUSE mount (Arcade Link 1.0.11).
- Release files are named `Arcade-Box_<version>_...` again, as in 0.x; 1.0.0-rc.1 was published under names that its checksum list and manifest did not match.
- Saving a result named after an input with a long CJK or emoji name no longer fails on the 255-byte file name limit.

## [0.4.3] - 2026-11-06

### Changed

- Clear inherited AppImage library/launcher paths when explicitly restarting Box or opening Arcade Tools from About.
- Make the catalog search shortcut open and focus Tools search from dashboard pages without a search field.
- Check real mouse-wheel and keyboard scrolling, bottom reachability, sidebar focus, page changes and search shortcuts across dashboard sizes and themes, including larger text. The native package gate checks scrolling in the real tool catalog and Settings too.

### Fixed

- Restore vertical scrolling in the dashboard content pane so the complete tool catalog and Settings remain reachable. Support keyboard scrolling and reset the pane to the top when changing pages or categories.

## [0.4.2] - 2026-11-05

### Changed

- Release closed Linux WebViews by disconnecting their retiring IPC callback, breaking Wry’s reference cycle without restarting the app. Native package checks require zero renderer processes after each close/reopen cycle.
- Gate the actual AppImage on three close/reopen cycles without a PID change; explicit Restart and Quit remain tested.

### Removed

- Remove the hidden 20-second memory-cleanup process restart after closing the Island. Release its WebView and reclaim heap pages in place, keeping background jobs, results, shortcuts and Link presence in the same resident process.

## [0.4.1] - 2026-11-03

### Changed

- Allow the dashboard to register the same trusted events as the Island, restoring Settings navigation and opening tools.
- Validate the real packaged dashboard → Island → calculation flow, plus Lua syntax, before release.
- Secondary labels and success, warning and error text use the shared readable
  foreground colors in both themes. This includes the Island search groups,
  dashboard sidebar, paths, onboarding and tool results.
- Pins Arcade Link and its design kit to v0.4.15 and adds `.editorconfig`.

### Fixed

- Restore the floating Hyprland Island and global shortcut registration by fixing Lua regex quoting.

## [0.4.0] - 2026-10-29

### Added

- New command-line flags: `--show`, `--hide`, `--toggle` and `--status`.

### Changed

- The dashboard is its own window: a normal, resizable window with a sidebar
  (Tools, Jobs, Engines, Pipelines, Plugins, Settings) that you can close
  without quitting Box. The Island stays the quick search overlay, and the
  first-run setup happens inside it.
- Box uses the shared Arcade design kit (Inter, the kit's colours, controls
  and icons) in the Island, the dashboard and the pinned-reference window.
  The tray icon is the monochrome Box glyph.
- Settings follows the shared order: General (start at login, save folder),
  Shortcuts, Clipboard history, Privacy, Connected apps and About (version,
  copy diagnostics, open logs).
- Box moves to the `dev.arcade.Box` identifier and Arcade's shared folders
  (`arcade/box`). The first start moves your data there, checks the copy and
  then removes the old folder. The desktop app and the `arcade-box` command
  line now share one database. On macOS the new bundle id means screen
  recording and accessibility permissions are asked for again.
- The default shortcut on macOS is ⌃⌥Space for new installs; existing users
  keep ⇧⌘Space.
- Releases are now made from a version tag (`tag.yml`, then `release.yml`);
  the assets are unchanged. The repository now includes the GPL-3.0 licence
  text.

### Fixed

- Fixed: the global shortcut stopped working on Wayland after it was saved
  (B1); unit symbols lost their case, so `1 mW to W` gave 1000000 W (B2);
  `in` (inch) was dropped as a connector word (B3); compound amounts such as
  `5 ft 11 in` and energy prefixes were missing (B4); the calculator couldn't
  call `log10` (B5); rounded result text changed the number (B6); Image
  convert and Favicon refused SVG files (B7); Batch barcodes refused CSV
  files (B8); multi-language OCR such as `eng+deu` was rejected (B9).
- Fixed: Image compare refused a PNG with transparency against its JPEG copy,
  and it reported every difference twice as large (up to 510 for 8-bit
  images). Screen tools run from the terminal now say they need the desktop
  app.

## [0.3.5] - 2026-10-19

### Changed

- The Island now takes keyboard focus when you open it with the shortcut on X11
  window managers that prevent focus stealing, such as Openbox. It opened
  without focus, so what you typed went nowhere and Esc did nothing until you
  clicked into it. The shortcut is caught outside GTK, so the Island carried an
  old user time; it is now shown with the X server's current time.

## [0.3.4] - 2026-10-19

### Changed

- The Island and Settings no longer crash Arcade Box when it runs from its
  AppImage (0.3.1 to 0.3.3). Since 0.3.1 Box moved to the folder you started it
  from so relative paths work in `arcade-box`, and the web view behind the
  Island and Settings then could not find its helper programs, which are looked
  up relative to the AppImage. The terminal tools still resolve paths against
  your folder; the desktop app goes back into the AppImage's folder first.
- The AppImage check in CI now opens Settings from the installed AppImage and
  fails if Box quits.

## [0.3.3] - 2026-10-19

### Changed

- A helper an app starts from its own image (such as a settings window) keeps
  working after the app quits: the previous release stopped passing the AppImage
  runtime's keep-alive pipe to every program, its own helpers included, so
  quitting could unmount the image under them. Now only programs from another
  image (the installed copy, other Arcade apps, a restart through the AppImage
  file) drop it, so installing from Downloads still leaves nothing behind.
- Restarting after the Island closes no longer keeps the previous image mounted.

## [0.3.2] - 2026-10-19

### Changed

- Installing or updating from your Downloads folder no longer leaves the
  downloaded AppImage mounted and its runtime running. The previous release
  stopped passing on the handle on the AppImage's mount, but the runtime also
  waits on a pipe the app inherits, and the installed copy still inherited that;
  now no inherited handle is passed on.
- `arcade-box --version` reports the app's version (it said `arcadebox 0.1.0`),
  and its help names the `arcade-box` command.

## [0.3.1] - 2026-10-19

### Changed

- Updating or installing from your Downloads folder, using the Island (Box
  restarts lean afterwards) and starting other Arcade apps no longer leave the
  old AppImage mounted and its runtime (about 20 MB) running. A program the app
  started inherited an open handle on its AppImage mount and kept it busy.
- `arcade-box` in a terminal accepts relative paths (`arcade-box run
  arcade.text.csv --file people.csv`); before, only absolute paths worked.

## [0.3.0] - 2026-10-18

### Changed

- Tools: colour conversion keeps alpha and accepts percentage RGB; CSV keeps its
  headers; big integers stay exact; XLSX export uses the right delimiters.
- Duplicate finder: overlapping folders are scanned once, several folders can be
  checked together, and one hash buffer is reused. Photo tools follow the EXIF
  orientation, and cover images keep their order. The image pipeline peaks at
  about 44 MB instead of 215 MB.
- The site check uses your system's DNS servers.
- Lower idle memory: WebKit's processes are shed after the Island closes;
  reopening takes about a second.
- The shortcut is shown as "Ctrl+Alt+B", Esc closes the first-run card, and logs
  are written to the folder Box advertises, also without a terminal.
- On Linux the `arcade-box` terminal command is installed with the app and removed
  when you uninstall it.
- The install prompt matches the other Arcade apps.
- Built against Arcade Link commit `2fc7c91`.

## [0.2.1] - 2026-10-15

### Changed

- Create the Island's WebView only when UI is needed (shortcut, tray "Open Box", Settings, a second launch, Link `app.activate` or `box.open`) and destroy it when the window is closed, or 15 s after the Island is hidden. A `--background` start no longer runs WebKit/WebView2 processes; it measured about 419 MiB RSS before (arcade-desktop 171 + WebKitWebProcess 184 + WebKitNetworkProcess 64 MiB). The tray, the global shortcut, the Link server and jobs such as `box:arcade.image.ocr` run in Rust without a WebView (SPEC §17.5).
- `box.open` requests that arrive while the Island is loading are delivered once it is ready.
- Closing the last window (the Island or a pinned image) no longer ends the resident process.

## [0.2.0] - 2026-10-14

### Added

- Add Arcade Link v0.3.0 hub methods, tray hosting and portable settings.
- Add receipt-owned installation, repair and removal, with Linux first-run setup and macOS Applications support.
- Add user guides, generated shortcut tables and CI checks with isolated profiles on Rust 1.88.

### Changed

- Use the shared per-user Windows installer integration and release tools from Link v0.3.0.

[Unreleased]: https://github.com/qa-p1/Arcade-box/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/qa-p1/Arcade-box/compare/v0.4.3...v1.0.0
[0.4.3]: https://github.com/qa-p1/Arcade-box/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/qa-p1/Arcade-box/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/qa-p1/Arcade-box/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/qa-p1/Arcade-box/compare/v0.3.5...v0.4.0
[0.3.5]: https://github.com/qa-p1/Arcade-box/compare/v0.3.4...v0.3.5
[0.3.4]: https://github.com/qa-p1/Arcade-box/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/qa-p1/Arcade-box/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/qa-p1/Arcade-box/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/qa-p1/Arcade-box/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/qa-p1/Arcade-box/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/qa-p1/Arcade-box/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/qa-p1/Arcade-box/compare/v0.1.2...v0.2.0
