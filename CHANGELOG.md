# Changelog

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
