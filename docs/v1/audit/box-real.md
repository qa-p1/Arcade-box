# Box 0.4.3: real-desktop audit (owner's Hyprland 0.56.2, 2560x1440)

Tested on the owner's real desktop on 2026-11-06. This covers what the isolated audit (`box.md`) couldn't reach.
Screenshots are in `box-real/`.

## How the desktop was driven safely

This works without root and never touches T3. The helpers are in the session scratchpad (`hk.sh`, `wlrctl-bin`).

- **Keys** go to one exact window by address, through Hyprland's own dispatcher:
  `hyprctl dispatch "hl.dsp.send_shortcut({ mods = 'CTRL', key = 'return', window = 'address:0x…' })"`.
  If the window no longer exists, Hyprland rejects the call, so stray keys can't land in another app.
- **Pointer:** absolute moves use `hyprctl dispatch "hl.dsp.cursor.move({ x = X, y = Y })"`. Clicks and drags use
  `wlrctl` (Wayland virtual-pointer protocol; no uinput or root needed), patched to add `pointer drag dx dy` (press,
  move in 20 steps, release). Clicks only land on coordinates inside the app's own window, taken from `hyprctl clients -j`.
- **Shortcuts** are triggered with `hyprctl dispatch "hl.dsp.global('dev.arcade.Box:arcade-box-toggle')"`, the same
  path a real keypress takes.
- **Tray menu** items are clicked over D-Bus: `busctl --user call org.kde.StatusNotifierItem-<toolspid>-1 /MenuBar
  com.canonical.dbusmenu Event isvu <id> clicked s "" 0`.
- **Screenshots:** `timeout 5 grim -g "X,Y WxH"`.
- **Apps** are started in their own scope (`systemd-run --user --scope --slice=app.slice …`) so they outlive the agent session.
- **Never** use `hyprctl dispatch` window-closing commands. Close apps with their own `--hide` or `--quit`, Escape, or the tray.

## Owner requirement (2026-11-06)

- **BR-0, Escape inside a tool goes back to the search.** Pressing Escape inside a tool (for example, while a download
  runs) must return to the Island's search so another tool can be used. It must never close the Island. Today it does
  close it (BR-5), and the owner has to click the back arrow with the mouse. Escape order: in a tool, back to search;
  in search with text, clear the text; in an empty search, close. Footer hints must match exactly. A running job keeps
  running and stays reachable from the search ("Recent" or "Jobs").

## Confirmed broken or wrong

- **BR-1, the global shortcut can open the Island but never closes it.** Tested via
  `hl.dsp.global('dev.arcade.Box:arcade-box-toggle')`, three presses three seconds apart. The first opens the Island
  focused; the second and third leave it open, still focused and mapped. The shortcut is named "Show or hide".
- **BR-2, the Island's invisible area captures the pointer.** The Island window is always 740x740 at (910,0). In search
  mode only the top ~170 px is drawn; the rest is transparent but still takes input. With `follow_mouse = 1`, hovering
  over that empty area over T3 focuses Box. Since the Island doesn't close on focus loss, a 740x740 square of whatever
  is underneath can't be clicked while the Island is open. The input region must match the drawn panel, or the window
  must resize to its content.
- **BR-3, no audio or video playback.** The Box log repeats `GStreamer element appsink not found. Please install it.`
  (20 times). Root cause: the AppImage sets `GST_PLUGIN_SYSTEM_PATH(_1_0)=<mount>/usr/lib/gstreamer(-1.0)` but bundles
  no plugins at all (0 `.so` files under `gstreamer-1.0`; only `libgstapp-1.0.so.0` is present). So WebKit can't
  play any media, which confirms B-27. Bundle the needed plugins (app, playback, audio/video convert and resample,
  coreelements, typefind, autodetect, pulse/pipewire sink, the needed codecs), or stop overriding the system plugin path.
- **BR-4, search fails on the queries people type** (confirms B-6 on the real desktop):
  - `kg to lb` ranks Colour Converter first, then Date & Time, Excel, Loan, audio tools; no unit converter is visible.
  - `6*7` shows only "Passport / ID Photo" and no calculator answer.
- **BR-5, Escape closes the whole Island from a tool view** (confirms B-2). The tool view's footer says "Esc Back",
  but one Escape closed the Island. In search, the first Escape clears the text and the second closes, while the hint
  says "Esc Close" both times.
- **BR-6, Enter vs Ctrl+Enter hints contradict each other** (confirms B-1). Settings → Island keys says "Run the tool:
  Enter". In a tool view the footer says "↵ Run" while the run button says "Ctrl ↵". Enter actually opens the
  highlighted tool, and only Ctrl+Enter runs it.
- **BR-7, Document to PDF rejects plain text and Markdown.** `.txt` and `.md` fail with "does not accept input type
  file/octet-stream". `.docx` converts fine with the built-in converter, with a clear warning about layout. Text and
  Markdown are the most common documents and must convert.
- **BR-8, the engine list is incomplete.** `arcade-box providers` lists only ffmpeg, qpdf and vips. Tools actually use
  `/usr/bin/tesseract` (OCR, version 5.5.3), `/usr/local/bin/yt-dlp` (downloads) and `zbarimg`. The Engines page and
  `providers` must show every engine a tool uses, with its version and whether it's missing.

## UX and polish

- **BP-1, output location is inconsistent.** Document to PDF writes next to the input file; every other tool tested
  writes to `~/.local/share/arcade/box/job-artifacts/`. Follow the "Save folder" setting everywhere.
- **BP-2, misleading error text.** A plain HTTP 404 in Media Downloader adds "Arcade Box does not bypass DRM, paywalls,
  or private-content restrictions." Show that only when the failure is actually an access restriction.
- **BP-3, OCR accuracy on small UI text.** Screen OCR misreads "Box waits in the background, ready for its shortcut" as
  "Box waits i the background, ready for ts shortut". Upscale small captures (for example 2x) before tesseract.
- **BP-4, two icon identities.** The install receipt lists icons for both `dev.arcade.Box` and the legacy
  `dev.arcadebox.app`. Retire the legacy name.
- **BP-5, the Island window is recreated on every show.** Each show gets a new window address, and opening Settings
  from the tray turned the previous Island window into the tiled "Dashboard" window (same address 0x5606488f3c00,
  floating → tiled 1228x1398). It works, but it's the same window-role confusion behind the owner's earlier
  "Island became a tiled window" report. Use separate, stable windows for the Island and the dashboard.
- **BP-6, a zombie Lens process.** After Box asked Lens for a screen selection, `[arcade-lens] <defunct>` stayed in the
  process list. The parent (Lens) doesn't reap a child. (Lens finding.)

## Verified working on the real desktop

- The Island is a floating, pinned window at the top centre (not tiled) and takes focus on show.
- The Island stays open when focus moves to another window, as the owner wants. Esc/Esc closes it.
- The Tools tray menu (D-Bus) lists every app with its actions, plus Start all / Quit all / Quit Tools.
  Box → Settings opens the dashboard on Settings; Box → Open shows the Island.
- Settings → Start at login: on writes a correct `~/.config/autostart/arcade-box.desktop` and the install receipt;
  off removes both.
- Clipboard suggestion: with text on the clipboard the Island suggests "Change Case".
- Screen OCR end to end: Box → Arcade Lens region picker (fullscreen, "Drag to select · M Measure · Esc Close") → drag
  → the text comes back in the Island, with Send to my devices, Add to Wheel and Add to Shelf.
- CLI with real files: Duplicate finder (correct groups by BLAKE3), Folder sizes (correct totals per folder and
  extension), CSV→Excel and Excel→CSV (3 rows round trip), Document to PDF for `.docx`, Image OCR (tesseract 5.5.3),
  Direct file downloader (6,823 bytes, HTTP 200), QR generator.

## Still to test on the real desktop

Colour picker (the Lens picker opened; the pick itself isn't confirmed), QR decode from screen, Ruler (Lens measure),
Screen recorder (start, stop, output file valid), Window Pin, Paste Plain, Clipboard history (enable, copy, list, pin,
clear, skip list), Check for updates, the user guide, audio playback after the GStreamer fix, Pipelines, Plugins,
Favorites, Recent, Jobs, the dashboard search (Ctrl+K and /), and theme switching (System, Light, Dark) on the real
compositor.

## Round 2 (real desktop, same day): results so far

Safety note on method: `hyprctl dispatch hl.dsp.global(...)` only sends the portal "Activated" event, never "Deactivated".
Box ignores every further activation until it sees a release (`shortcut_down` in `wayland_shortcut.rs:398-401`), so
**BR-1 is probably an artefact of the synthetic dispatch, not a real bug** (a physical key press sends both). Please
treat BR-1 as unconfirmed; it still needs one physical Ctrl+Alt+Space test by the owner. Robustness suggestion: reset
`shortcut_down` after ~1 s (or when the Island is hidden by any other path) so a lost release can never wedge the shortcut.
Also: `wlrctl keyboard type` with ctrl+alt did not trigger the bind either, so real shortcut presses could not be simulated.

### New confirmed findings

- **BR-9, Window Pin always fails on this desktop** (broken). Steps: focus a floating window (Look), `--show` the Island,
  Window Pin tool, Ctrl+Enter, then click "Keep active window on top". Expected: Look gets pinned. Actual: a red card
  "Couldn't finish: No other window is active" shown at the same time as a green "Done, 1 result" card; no window is pinned,
  and the Island stays open. Also confusing: the button says "Toggle always on top" but only "prepares"; a second click on
  "Keep active window on top" / "Remove always-on-top" does the work. Source: `apps/desktop/src-tauri/src/hyprland.rs:168-190`
  (`foreign_active_window` reads `hyprctl activewindow` right after hiding; the Island is still the active window or none).
  Evidence: `real/box/pin-result2.png`, `pin-r5.png`. Same helper backs Paste Plain, so that depends on focus returning too.
  With a real pointer the focus may land under the cursor instead (possibly T3 when the cursor is over it), which makes
  "the window that was active" unpredictable. Pinning a tiled window floats it (documented in the tool text), which is surprising.
- **BR-10, Screen recorder cannot run** (broken). Island > Screen Recorder shows "Screen recording unavailable: needs
  compatible GStreamer pipewiresrc, vp8enc and webmmux elements" and a disabled "Start recording". The same three elements ARE
  installed on this system (`gst-inspect-1.0 pipewiresrc vp8enc webmmux` all found), so this is the AppImage forcing its own
  empty GStreamer plugin path (same root cause as BR-3). The tool text also says Wayland "uses the native picker", which is not
  what happens. Evidence: `real/box/rec-tool.png`.
- **BR-11, search highlight follows the stale pointer; keys are overridden** (confirms B-5 on real desktop). After the pointer
  rested over the result list, typing `paste plain` highlighted the LAST row ("Webpage to Text") and Enter opened it instead of
  the first row "Paste as Plain Text". Even 5x Up did not reach the first row (Down from the last row wrapped to the first).
  Evidence: `real/box/pp-list.png`, `pp-list3.png`.
- **BR-12, old query and old tool survive a re-show; first keystrokes lost** (UX). The Island re-opens on the last tool
  (e.g. Window Pin with a stale error card) or on the old query (`color pick`, `ruler`), so the first typed text is appended
  to it ("color pickqr", "rulerrecord"). Keys sent immediately after `--show` (<1.5 s) were partly lost.
  Expected: a fresh search each time (or a selected old query). Evidence: `real/box/qr-search2.png`, `rec-search.png`.
- **BR-13, Colour picker result is not returned to Box** (UX). Box > Pick screen color hides the Island and hands the whole
  job to Lens' own popup (HEX, RGB, Save, Pin ...). Lens copies the value, the Island never comes back, and when it is
  re-opened it says "Done, 0 results, Arcade Lens is open". The Screen OCR and QR flows DO return their result to the Island.
  Inconsistent. The picked colour itself was correct (#191A20 = grim pixel 25,26,32).
- **BR-14, Jobs page lists failures with no reason** (UX). Failed/Cancelled jobs show only "Finished" + Retry. Show the error.
  Evidence `real/box/page-jobs.png`.

### Verified working (round 2)

- Dark/Light/System appearance on the real compositor: whole dashboard re-themes at once, text readable in light and dark.
  Polish: the window title bar stays dark in Light theme (`real/box/theme-light.png`).
- Tools, Favorites (empty state), Recent (8 tools), Jobs, Pipelines (empty state), Plugins (empty state), Engines pages render.
- Screen QR / Barcode Scanner: displayed a generated QR (`ARCADE-AUDIT-QR-12345`) in Look, picked it with the Lens region
  selector, decoded correctly in the Island ("Links stay unopened until you choose Open reviewed link" is a good touch).
- Screen Ruler: Lens measure mode, drag 300x200 reported 299x203 (it snaps to window edges, also shows `W 1040 H 43` for
  a hovered window), Esc closes it and returns focus to the previous window.
- Paste as Plain Text: hides the Island and pastes into the previously focused window (a floating test terminal got the text).
  It targets whatever Hyprland focuses after the hide, so keep the pointer over the target.
- Clipboard History tool: opens the Arcade Clipboard picker ("Choose a clip for Arcade Box") listing the last items.
  This is the Clipboard app, NOT Box's own history, so Settings > "Keep clipboard history" (which showed "0 items saved" even
  after 3 copies) is a second, separate store. Two clipboard histories is confusing; consider one.

## Round 3 — continued real-desktop audit

- **BP-7 (UX), pipeline input choices expose raw internal MIME names.** New pipeline → Input type contains `text/plain;hint=command`, `structured/yaml`, `clipboard/item`, etc. Expected user labels (Text, URL, Image…) with advanced details separately. Actual requires knowledge of Link types before the first tool can be selected. Evidence: `real/box/pipeline-hash-final.png`. Source: `apps/desktop/frontend/src/lib/PipelinesView.svelte:544` renders `{choice}` directly.

Verified working:
- A saved two-stage pipeline, Hash / Checksum (SHA-256) → Change Case (UPPERCASE), maps `audit` to `B81F37A043A6F767E7C94D105F4BD31282F3ECC20680BB9D09BD93461CF4C863`, exactly matching Python hashlib. Save, edit and run work; it registers as the 110th searchable tool. Evidence: `real/box/pipeline-actual-output.png`.
- Ctrl+K opens dashboard catalog search from Settings, and selects the existing query once its asynchronous focus step has settled. Earlier blank/unchanged screenshots taken before mapping/repaint are not failures.
- Plugins empty state and native Choose plugin folder dialog open; Escape cancels. No plugin was installed (owner forbids installs).
- Box opt-in history DOES capture text on this compositor (1 entry in its JSON after >1.2 s). The Settings count is a snapshot; leaving/reopening Settings is required to refresh. The earlier “0 after copies” observation alone did not prove capture broken. History was OFF again after this check.

### BR-15 — broken — Direct downloader discards a complete file when HEAD is unsupported
Steps: local harmless HTTP fixture implements GET 200, Content-Length 262144, streaming 32×8192 bytes over 32 s; HEAD gets the standard Python HTTPServer 501 error (Content-Length 482). Island → Direct File Downloader → URL `http://127.0.0.1:18771/arcade-audit-download.bin` → Ctrl+Enter. Back to search, hide/reopen Island, then Dashboard → Jobs. Expected: use the successful GET response size and keep all 262144 bytes. Actual: `The server reported 482 bytes but the staged download contains 262144; the incomplete file was discarded`. Progress already looks nearly full (0.97) at 2 s because the wrong 482-byte HEAD length is used. Evidence: `real/box/job-running-before-back.png`, `job-dashboard-after-reopen.png`; reproducible fixture `real-audit/slow-http.py` (private session scratch). Source: `crates/arcade-core/src/web.rs:431-470` `probe_content_length` accepts curl success without checking HTTP status (`--fail` absent); lines 196-199 only replace its length with GET headers when total==0. Failed HEAD must not supply a length, and GET headers should be authoritative.

- Background-job lifetime: the slow job survives Back, hiding the Island and reopening it; it stays reachable on Dashboard → Jobs. The reopened empty Island has no running-job indicator or link (owner wants jobs reachable from search too). `real/box/job-after-back.png`, `job-reopened.png`.
- Favorites: Hash / Checksum can be starred with keyboard Tab/Enter, then appears in Dashboard → Favorites (1 item). `real/box/favorite-added.png`.

- Box history full fallback cycle works after temporarily disabling its Clipboard peer: enable, capture a unique audit text, list, pin, clear (including pinned), disable. Peer restored ON and exclusions remain empty. On Wayland the skip-app list explicitly says source detection is unavailable; no claim that it enforces exclusions here. `real/box/own-history-fallback.png`, `history-pin-focus.png`, `history-pinned.png`.
- **BP-8 (UX), clearing history from the Island has no confirmation.** Unlike Settings → Clear… (explicit confirmation), the fallback tool’s “Clear history” immediately deletes every item, including pinned ones, with no confirm or undo. Tested only with the audit entry; owner history was empty. Source: `frontend/src/lib/ClipboardHistoryView.svelte:115,156`.
- Guide action launches the default browser; the new audit browser window was closed afterwards. With Tools connected, the update action correctly becomes “Updates are managed by Arcade Tools” / Open Tools rather than a second updater. The source’s fallback “Check for updates” opens Tools’ GitHub releases URL.

### BR-16 — broken — A successful download with no Content-Type is rejected
Follow-up to BR-15: same local 262144-byte stream, now HEAD 200 with correct length, GET 200 with correct length, neither response sends Content-Type. CLI `arcade-box run arcade.web.file-downloader http://127.0.0.1:18772/arcade-audit-good.bin --output-name arcade-audit-good.bin --json` completes the transfer but exits 1: `curl completed without download metadata`. Expected: accept the file as application/octet-stream. Evidence: private scratch `download-good-stderr.txt`, `slow-http-good-head.py`. Source: `crates/arcade-core/src/web.rs:518-522`: `metadata.trim()` removes the trailing tab for empty `%{content_type}`, so splitting yields 3 fields and the parser rejects it. Keep empty last field / trim only newlines. A third control with Content-Type supplied is tested separately.
