# Arcade Box 0.4.3 audit (pre-1.0.0)

Date 2026-11-06. Installed AppImage `~/Applications/Arcade/Arcade-Box.AppImage` (0.4.3), run only in an isolated session (private HOME/XDG dirs, Xvfb :91, own dbus, no window manager). Nothing was edited, committed or installed. Owner processes untouched; all my processes (Xvfb, app, dbus, portals, WebKit) were stopped at the end.

Methods used:
- **GUI** = real AppImage in Xvfb driven by xdotool, screenshots via `import`.
- **CLI** = `usr/bin/arcade-box` extracted from the same AppImage (0.4.3, same Rust core as the GUI; `run`, `search`) with real inputs.
- **SRC** = read source to explain a finding.
Limits: no window manager, so focus-loss/auto-hide, window placement and tray were not testable; no Wayland/portal; no real clipboard. CLI passes `--set k=v` as JSON when it parses (see U-1).

Evidence is in `box/` (screenshots, `cli-evidence.txt`).

## Coverage

Idle/process: main process 0.27% CPU, 71 threads steady, RSS 220 MB; two WebKit web processes 265-282 MB each (about 830 MB total) over 30 s with Island hidden and dashboard open. No threads/fds growth. No writes outside `data/arcade/box`, `cache/arcade/box` (plus `cache/mesa_shader_cache`, from Mesa). `config/arcade/box` was never created. No `logs/` dir appeared in the isolated data dir (stderr only).

| Area / tool | Method | Inputs | Result |
|---|---|---|---|
| Island open, search, Esc, mouse leave | GUI | typed queries, Esc, mouse away 6 s, click in transparent area | Pass for stay-open; fail: B-1, B-2, B-5 |
| Island tool run (Calculator, Unit, Timer, Screenshot, Image Resize incl. file picker) | GUI | 6*7, 14 GB to MB, 3 s timer, area capture, -5 / 100.5 / 200 width | Pass, with B-1, B-3, B-4, B-14 |
| Dashboard: Tools, Favorites, Recent, Jobs, Pipelines, Plugins, Engines, Settings, search Ctrl+K, wheel scroll, dark/light | GUI | all pages, theme Dark, search "unit" | Pass; B-13 (button wrap), B-12 (Engines) |
| Settings controls (theme tested; start-at-login, Check updates, Open guide untested) | GUI | theme | partial |
| Global shortcut / tray / onboarding shortcut | GUI (Xvfb) | n/a | shortcut shows raw ZBus error (B-15, env-specific); tray untestable |
| Search ranking, 90 queries | CLI + GUI | words, natural phrases, expressions | B-6 |
| convert.calculator | CLI | 30 expressions | B-7 |
| convert.percentage / loan | CLI | all modes, negatives, 0, huge, 2.5 people | Pass; B-18 |
| convert.units | CLI | 50 inputs | B-8 |
| convert.currency | CLI (network) | 100 USD to INR, ZZZ, negative, 1e12, empty | Pass |
| convert.time-zone / date-duration | CLI | DST gaps, abbreviations, leap, reversed | B-9, B-17 |
| convert.color | CLI | 20 formats | B-17 |
| convert.excel-csv | CLI | a.csv | Pass |
| text.case / clean / structured / csv / diff / markdown / statistics / lorem | CLI | unicode, empty, CRLF, ragged CSV, big ints | B-10, B-11, B-16, B-19 |
| utility.emoji / random / timer / security.password | CLI + GUI(timer) | edge counts, empties | Pass; B-20 |
| barcode QR / barcode / decode / batch | CLI | all 9 symbologies, round-trip decode | B-21 |
| developer regex / encoding / url / hash / identifier / jwt / cron | CLI | edge inputs | B-22 |
| files rename / compare / encrypt; duplicates, sizes | CLI (rename, compare, encrypt); folder tools untested | | Pass |
| pdf merge/split/organize/compress/pdf-to-images/extract/ocr/protect/watermark/fill/images-to-pdf/sign | CLI | valid, fake, empty, out-of-range pages | B-23; pdf.convert untested (needs Office engine) |
| image convert/resize/crop/compress/combine/watermark/favicon/redact/metadata/ocr/palette/compare/passport | CLI | valid plus bad numbers | Pass; B-24 |
| image background-remove, upscale; audio vocals; auto-subtitles, transcribe | CLI | default | Fail by design without engines/keys (B-12) |
| video convert/compress/trim/crop/join/extract/gif/frames/subtitles/inspect | CLI | valid, out-of-range | Pass |
| audio convert/trim/join/normalize/denoise/speed-pitch/silence/metadata/tts | CLI | valid, out-of-range | Pass |
| network site-check / rdap / ip; web markdown/file-downloader/images/snapshot/thumbnail/transcript/downloader | CLI (network, one request each) | example.com, bad hosts, file://, localhost | Pass; downloader/thumbnail/transcript untestable on example.com |
| system.system-info, process | CLI | | B-25 (process) |
| screen.screenshot | GUI | area drag | B-26 |
| screen.color / ruler / ocr / qr / recorder / pin; system.clipboard-history / paste-plain / window-pin | not completed (need real Wayland/clipboard/compositor) | | untested |

## Findings

### B-1 [UX] Footer and Settings say Enter runs a tool; it does not
Steps (GUI): Island, open Calculator, type `2+2`, press Enter. Expected: runs (footer reads "Enter Run", Settings > Island keys "Run the tool: Enter"). Actual: nothing runs; only Ctrl+Enter runs (the button says Ctrl+Enter). Evidence: `island-esc-and-stale-result.png` (old 6*7 result still shown after Enter), `settings-island-keys.png`. Source: `frontend/src/island/IslandFooter.svelte:14`, `dashboard/SettingsPage.svelte:24`, `box.svelte.ts:779`.

### B-2 [UX] Esc in a tool closes the whole Island, but the footer says "Back"
Steps (GUI): open any tool, press Esc. Expected "Back" to search per footer and Settings ("Back, or close"). Actual: Island hides (state kept for next show). Source: `box.svelte.ts:~789` (Escape always `dismissIsland`; back is Alt+Left).

### B-3 [UX] Island footer clipped once a result is shown
Steps (GUI): Calculator, 6*7, Ctrl+Enter. The window is 740 px tall; the card scrolls and the key-hint row at the bottom is cut in half. Evidence: `island-footer-clipped.png`. Source: Island height cap (`lib.rs:1054`, `.island-shell`). Seen without a WM only; check on Hyprland.

### B-4 [UX] Stale result stays visible after the input changes
Calculator shows the 6*7 = 42 card while the input says 2+2. Evidence: `island-esc-and-stale-result.png`. Minor.

### B-5 [UX] Search results highlight follows a stale mouse position
With the pointer resting over the list, typing `calc` + Enter opened Time Zone Converter (row under the pointer, `onmouseenter`) rather than Calculator (first row). Evidence: `island-search-enter-opens-wrong-tool-hover.png`. Source: `island/IslandSearch.svelte:onmouseenter`. Selection should only follow real pointer movement.

### B-6 [wrong result] Island search cannot find tools from what users type
Steps (GUI+CLI): `kg to lb` -> CSV Workbench first, Unit Converter not on first screen; `14 gb to mb`, `72 f to c` -> text tools; `6*7` -> Passport / ID Photo only; `100 usd to inr` -> Image Compress first; `#ff8800`, `2026-01-01` -> nothing; `ip` -> image.convert, rdap, percentage... (My IP not in top 6); `remove background` -> audio.denoise first; `resize` -> image.compress first; `kg` -> Background Remove. Expected: expression/unit/colour/date inputs route to the matching tool; names rank first. Evidence: `island-search-kg-to-lb-and-6x7.png`. Source: tool search scoring in `arcade-core` (fuzzy token matching, no input classification).

### B-7 [wrong result] Calculator
CLI. `99999999999999999999*99999999999999999999` shows `10000000000000000303786028427003666890752` (float noise digits; should be 1e40 style). `sin(30)` = -0.988 (radians only, no degree toggle, nothing says so). `100+10%`, `5!`, `1,000+2`, `2**3`, `10 mod 3`, `50% of 80` unsupported; messages: "Unexpected character at byte 1", "Unknown constant `mod`", empty -> "Expected a number or function at byte 0". `%` is modulo, not percent. Source `arcade-core/src/converter_tools/calculator.rs`.

### B-8 [wrong result] Unit Converter
CLI. `1,5 m to cm` -> `15 m = 1500 cm` (decimal comma silently dropped; wrong). `1.5e3 m to km` / `1e3 m` -> "Unknown unit `e3`". No time (day, year), no light-year, Mbps, degrees/radians (all "Unknown unit"). `5 m cm` / `5 m to` -> "Enter a numeric amount" (misleading). `1 knot` = 1.8519999999983998 km/h internally (factor imprecise). Source `converter_tools/units.rs`.

### B-9 [wrong result] Date & Time / Time Zone
CLI. `difference` when end is before start: `totalDays` is -358 but `totalHours`, `totalMinutes`, `totalSeconds`, `weekdays` are positive (8592...). Time Zone converter rejects `9:00`, `tomorrow 5pm`, Unix timestamps (which Date tool accepts), and `IST/PST/EST`; `utc`/`asia/kolkata` lower-case rejected. Source `converter_tools/datetime.rs`.

### B-10 [wrong result] Text diff reports differences between identical texts
Steps (CLI, same core as the Island): input `a\n--- RIGHT ---\na\n`. Expected: no differences. Actual: `-a` / `\ No newline at end of file` / `+a`, and in `a b c` vs `a B c d` the unchanged `c` line is shown as changed. Cause: the left text loses its trailing newline at the separator, the right keeps its own. Side-by-side/inline views mark `c` as `~`. CRLF input gives a garbled `-b\r+b\r` diff. Evidence `cli-evidence.txt`. Source `arcade-core/src/text_tools.rs:447-455` (`split_diff_input`). Also UX: the tool is one text box with a magic `--- RIGHT ---` line.

### B-11 [wrong result] Text tools
CLI. Title case: `2nd` -> `2Nd`; final sigma `ΣΑΣ` -> `Σασ`. camel/snake split contractions (`it's` -> `itS`, `it_s`). JSON/YAML/TOML conversion turns big integers into floats (`12345678901234567890123` -> `1.2345678901234568e+22`); duplicate JSON keys silently dropped; invalid JSON says "Couldn't tell which format this is" instead of the syntax error. CSV: rename mappings with commas (`name=Name,age=Age`) produce header `"Name,age=Age"` with no error (one-per-line is required; the catalog control is a single-line `text` type, so confirm in the GUI that multiple mappings can be entered); `.json` input and empty files are refused with "does not accept input type file/octet-stream" via CLI. Source `text_tools/clean.rs`, `structured.rs`.

### B-12 [UX] Engines page omits optional engines; several "implemented" tools just fail
Engines shows only FFmpeg, libvips, qpdf, Tesseract ("4 of 4 found"). Background Remove (rembg), Upscale (Real-ESRGAN), Vocals (Demucs), RNNoise, Auto-Subtitles/Transcribe (Groq key), Passport background replacement, Document-to-PDF, yt-dlp tools depend on other engines/keys that are not listed, so users get a one-line failure with no place to see or fix it. Evidence `engines-page.png`; CLI messages in `cli-evidence.txt` session. Source `frontend/src/dashboard/EnginesPage.svelte`.

### B-13 [polish] Pipelines "New pipeline" button wraps
The "+" sits above the text in a narrow button. Plugins "Choose plugin folder" has the same wrap. Evidence `pipelines-button-wrap.png`.

### B-14 [polish] Raw MIME in tooltip and clipped validation text
Hovering the chosen file shows a tooltip `file/image`; inline validation line ("Width must be at least 1.") touches the card edge. Evidence `image-resize-validation.png`.

### B-15 [UX, env-specific] Onboarding and Settings show a raw ZBus error
Without an installed `.desktop` file the shortcut row prints `Could not register portal identity ... ZBus Error: org.freedesktop.portal.Error.Failed ...` in both the welcome card and Settings. Raw error text for end users. Source `src-tauri/src/wayland_shortcut.rs` status text.

### B-16 [UX] Text statistics / readability for non-English
Flesch score 115.6 for a Japanese sentence; words 13 for unspaced text. The note admits it is approximate.

### B-17 [polish] Colour converter errors
`notacolor`, `#gggggg`, `#12`, `transparent` -> "`...` is not a number". `cmyk(...)`, `oklch(...)` unsupported.

### B-18 [polish] Percentage / discount input handling
People=2.5 silently becomes 3; discount accepts negative price despite `minimum: 0`; change mode prints a Unicode minus (`−25%`) while other modes print `-`.

### B-19 [polish] Lorem count 1.5 silently truncates; `count=0` error is clear.

### B-20 [wrong result] Password tool
PIN strength check warns "Short keyboard patterns are easy to guess" and suggests "a longer keyboard pattern" for a random PIN; `Tr0ub4dor&3` is "very strong" with offline crack time "3 months"; "passphrase" mode generates pseudo-words (`staut.keep.neam.naib.fid`), not real words. Source `security_tools.rs`.

### B-21 [UX] Barcodes
EAN-13/UPC-A/UPC-E silently zero-pad short input and append a check digit (`12345678` as EAN-13 decodes as `0000123456784`; `590123412345` -> `5901234123457`). EAN-8 `12345678` gives "Invalid EAN-8 check digit '8', expecting '0'". Errors expose ZXing internals ("Error 284 ... (retval: 6)", "Error 567: Input too long ... requires 8003 codewords"). Source `arcade-core/src/barcode.rs`.

### B-22 [polish] Developer tools
Regex: `g` flag has no effect (all matches always), invalid flags (`q`) accepted, engine has no look-around/backrefs (disclosed in output). Hash: no MD5/SHA-1. Base64 decode rejects unpadded and URL-safe input; hex decode rejects `0x` prefix; JWT rejects a `Bearer ` prefix; URL tool errors "relative URL without a base" for `example.com/path` or empty; URL decode turns `+` into space. Cron: no names (`MON-FRI`, `JAN`), "description" only restates the fields; `0 9-17/2 * * MON-FRI` errors.

### B-23 [polish] PDF
`pdf.protect` unlock on an unencrypted PDF reports "Removed PDF encryption using the supplied valid password". qpdf errors leak the temp path and qpdf wording ("parsing numeric range for /tmp/.../multi.pdf: error at * in numeric range *0-2"). Watermark rejects non-ASCII text. UI defaults differ from backend defaults (compress UI default targetSize 2 MB; CLI default is lossless; image convert UI default webp, CLI default png).

### B-24 [UX] Image combine "same size" shrinks everything to the smallest image
Three images (800x600, 800x600, 64x64), grid, defaults -> result 128x128. Source `arcade-core/src/image/compose.rs` (or magick.rs).

### B-25 [broken] Process Inspector lists threads as processes
CLI `arcade.system.process quickshell` -> 57 "processes" named `QDBusConnection`, `QNetworkAccessM`, `QQmlThread`; empty query -> 1622 matches while `ps -e` shows 406 processes (`ps -eL` 1727 threads). Expected: one row per process. Evidence `cli-evidence.txt`. Source `arcade-core/src/system_tools*` (sysinfo process list, need to skip tasks/threads).

### B-26 [wrong result, X11 only] Screenshot area capture includes the selection overlay tint
GUI under X11 (the x11-overlay path when the portal is missing): drag-select over a dark window; saved `screen-selection.png` pixels are (22,47,76) where the screen is (22,24,29): the overlay's blue fill was captured. Evidence `screen-selection-capture.png`, `screenshot-tool-x11.png`. Source `src-tauri/src/screen_capture.rs` (overlay must be unmapped before grabbing).

### B-27 [broken, likely] Audio/media playback unavailable in the AppImage
Log lines: `GStreamer element appsink not found`, `autoaudiosink not found`, then `GStreamer-CRITICAL gst_bin_add` assertions in WebKitWebProcess when the Timer view is opened (also in the owner's real log, 19 times). The WebKit bundle has no GStreamer plugins, so the timer alert sound and the audio/video preview in the media tools cannot play. Whether anything is audible was not verified (Xvfb has no audio).

### U-1 (core) [wrong result via CLI/Link] Numeric option values for string options are silently ignored
`--set tabs=4` (text clean) and `--set indent=4` (structured) are ignored with no error; `--set range=5` (PDF split) fails with "Page range must be text"; `--set pageOrder=9` (PDF organize) succeeds with no effect. Via `--options-json '{"tabs":"4"}'` they work. GUI controls send strings so the UI is likely fine; Link callers and the CLI are affected. Source: option parsing in `arcade-core` (`option_str`).

## Unconfirmed / suspicions
- Island window position changed between shows (x=960, 590, 590), and the first show was at 960. Could be Xvfb/no-WM only.
- Dark-theme switch looked unapplied in two screenshots taken 1-2 s later (a later grab was dark); probably Xvfb repaint timing, not reproducible deterministically.
- `shortcut status: invalid_trigger` appears 9 times in the owner's real log (older builds). `validate_trigger` accepts the portal spelling the recorder produces, so probably fixed in 0.4.x; not retested (no portal here).
- Focus-loss auto-hide, tray menu, Start-at-login toggle, Check for updates, user guide, global shortcut, clipboard suggestions and history, Paste Plain, Window Pin, Recorder, Ruler, Screen OCR/QR/Color (needs real compositor), Folder tools (duplicates, sizes), Excel input, doc-to-PDF, yt-dlp downloads: not exercised.
- Memory: about 830 MB RSS for a tray resident (main 220 MB + two webviews).
- "Value" and "Formatted" rows are redundant for Calculator results (polish).
