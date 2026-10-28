# Changelog

## 0.4.0 — 2026-10-28

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
- New command-line flags: `--show`, `--hide`, `--toggle` and `--status`.
- Fixed: the global shortcut stopped working on Wayland after it was saved
  (B1); unit symbols lost their case, so `1 mW to W` gave 1000000 W (B2);
  `in` (inch) was dropped as a connector word (B3); compound amounts such as
  `5 ft 11 in` and energy prefixes were missing (B4); the calculator couldn't
  call `log10` (B5); rounded result text changed the number (B6); Image
  convert and Favicon refused SVG files (B7); Batch barcodes refused CSV
  files (B8); multi-language OCR such as `eng+deu` was rejected (B9).
- Releases are now made from a version tag (`tag.yml`, then `release.yml`);
  the assets are unchanged. The repository now includes the GPL-3.0 licence
  text.

## 0.3.5 — 2026-10-19

- The Island now takes keyboard focus when you open it with the shortcut on X11
  window managers that prevent focus stealing, such as Openbox. It opened
  without focus, so what you typed went nowhere and Esc did nothing until you
  clicked into it. The shortcut is caught outside GTK, so the Island carried an
  old user time; it is now shown with the X server's current time.

## 0.3.4 — 2026-10-19

- The Island and Settings no longer crash Arcade Box when it runs from its
  AppImage (0.3.1 to 0.3.3). Since 0.3.1 Box moved to the folder you started it
  from so relative paths work in `arcade-box`, and the web view behind the
  Island and Settings then could not find its helper programs, which are looked
  up relative to the AppImage. The terminal tools still resolve paths against
  your folder; the desktop app goes back into the AppImage's folder first.
- The AppImage check in CI now opens Settings from the installed AppImage and
  fails if Box quits.

## 0.3.3 — 2026-10-19

- A helper an app starts from its own image (such as a settings window) keeps
  working after the app quits: the previous release stopped passing the AppImage
  runtime's keep-alive pipe to every program, its own helpers included, so
  quitting could unmount the image under them. Now only programs from another
  image (the installed copy, other Arcade apps, a restart through the AppImage
  file) drop it, so installing from Downloads still leaves nothing behind.
- Restarting after the Island closes no longer keeps the previous image mounted.

## 0.3.2 — 2026-10-19

- Installing or updating from your Downloads folder no longer leaves the
  downloaded AppImage mounted and its runtime running. The previous release
  stopped passing on the handle on the AppImage's mount, but the runtime also
  waits on a pipe the app inherits, and the installed copy still inherited that;
  now no inherited handle is passed on.
- `arcade-box --version` reports the app's version (it said `arcadebox 0.1.0`),
  and its help names the `arcade-box` command.

## 0.3.1 — 2026-10-18

- Updating or installing from your Downloads folder, using the Island (Box
  restarts lean afterwards) and starting other Arcade apps no longer leave the
  old AppImage mounted and its runtime (about 20 MB) running. A program the app
  started inherited an open handle on its AppImage mount and kept it busy.
- `arcade-box` in a terminal accepts relative paths (`arcade-box run
  arcade.text.csv --file people.csv`); before, only absolute paths worked.

## 0.3.0 — 2026-10-18

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

## 0.2.1 — 2026-10-15

- Create the Island's WebView only when UI is needed (shortcut, tray "Open Box", Settings, a second launch, Link `app.activate` or `box.open`) and destroy it when the window is closed, or 15 s after the Island is hidden. A `--background` start no longer runs WebKit/WebView2 processes; it measured about 419 MiB RSS before (arcade-desktop 171 + WebKitWebProcess 184 + WebKitNetworkProcess 64 MiB). The tray, the global shortcut, the Link server and jobs such as `box:arcade.image.ocr` run in Rust without a WebView (SPEC §17.5).
- `box.open` requests that arrive while the Island is loading are delivered once it is ready.
- Closing the last window (the Island or a pinned image) no longer ends the resident process.

## 0.2.0 — 2026-10-14

- Add Arcade Link v0.3.0 hub methods, tray hosting and portable settings.
- Add receipt-owned installation, repair and removal, with Linux first-run setup and macOS Applications support.
- Use the shared per-user Windows installer integration and release tools from Link v0.3.0.
- Add user guides, generated shortcut tables and CI checks with isolated profiles on Rust 1.88.
