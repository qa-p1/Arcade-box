# Arcade Link

Arcade Box is the transform engine of the Arcade apps. Through
[Arcade Link](https://github.com/qa-p1/Arcade-link) the other apps (Lens,
Look, Wheel, Clipboard) run Box's tools on their content without
reimplementing them. Box works exactly the same when no other Arcade app is
installed.

## What Box exposes

| Action | Meaning |
|---|---|
| `box:<tool-id>` | Every implemented tool, for example `box:arcade.image.convert`. Accepts the tool's input types in Link form (`file/image`, `text/plain`, …); Box's screen and clipboard inputs aren't offered (Lens and Clipboard own those verbs). |
| `box:<tool-id>#<preset>` | A preset: a named option set from `catalog/tools.json`, e.g. `box:arcade.video.compress#share-25mb` ("Compress for sharing"). These are what other apps show as one-click entries. |
| `box.open` | "More in Arcade Box…": opens the Island with the input attached (`options.tool` opens that tool directly). |
| `box.pipelines` | Returns a `structured/pipelines` array of available saved pipelines: `{id, name, version, accepts, produces, effects, interactive}`. |
| `box.pipeline.run` | Runs the saved pipeline named by `options.pipeline`, with its first stage's input in `inputs`, through the same job manager and delegated grants as a tool. |

`link.featuredFor` in the catalog marks which presets a peer shows inline
for a content type (3–5 per type); the rest go under "More in Arcade Box…".

Availability comes from a cached provider check (`link_provider_cache` in the
settings table), refreshed in the background at every start and whenever the
Engines page checks providers, so other apps never trigger a probe. A tool
whose engine is missing is listed with `available: false` and the reason
("FFmpeg isn't installed"); peers hide it.

Pipeline effects are the union of their stages' effects, and output types
come from their exposed output stages. Pipelines with unavailable engines,
unsupported sources or plugins are excluded from the consumer list; an
explicit run still returns the reason. Saving or deleting a pipeline
refreshes the manifest and emits `app.changed`. Peer nodes store `link: {app, action, version}` instead of `toolId`. Their
actions must be installed, enabled and available, and the saved action version
must match. The editor marks changed versions **Needs repair** and lets the
user select the current version. Interactive actions are allowed only at the
first stage. Pipelines containing `sends-to-device`, `network` or
`executes-commands` ask for confirmation before their first run; an edit or a
change to their effects invalidates that approval. Resident peer jobs end locally
on a cancellation acknowledgement and have a five-minute deadline, so a picker
that stays open cannot hold Box's job directory indefinitely. CLI users approve explicitly
with `arcade-box pipeline run <id> --confirm-effects`. One-shot requests reject
interactive pipelines and unapproved effects.

Intermediates and scoped grants live in a private per-run directory, removed
on success, failure or cancellation. Only declared output stages publish files;
only peer-bound file inputs are copied into Link handoff. User-selected source
files are copied before processing and remain unchanged.

```json
{"action":"box.pipeline.run","options":{"pipeline":"upper-clean"},"inputs":[{"type":"text/plain","text":"hello"}]}
```

## What Box consumes

| Peer action | Box surface / behavior |
|---|---|
| `lens.capture` | Local screen tools and an interactive pipeline first stage; returns a selected region image. |
| `lens.capture_and_act` | Ruler, image pin and color tools forward `options.mode` as `measure`, `pin` or `color`. |
| `lens.recognize` | `ocr.lens`, requesting `ocrOnly: true`, with provider provenance. |
| `lens.pin` | Pin a single image from the first result action row. |
| `clipboard.pick` | Choose a history item for Box without writing the OS clipboard. |
| `clipboard.add` | Confirmed Send to my devices ↗, and pipeline sinks; 16 MiB maximum, with Clipboard Private mode enforced by its owner. |
| `look.preview` | Preview output files in Arcade Look. |
| `wheel.add_action` | Add a tool, preset or saved pipeline; Wheel asks the user to choose a slot and confirm. |
| `shelf.add` | Add to Shelf from the first result action row: output files as one `file/any[]` reference batch, folders as `folder/reference`, and text as `text/plain`/`text/url`/`text/rich`. Shelf keeps references; Box's originals are never copied or moved. |
| `tools.install` | Connected apps Get buttons pass `options.app`; when Tools is absent, Get opens the app's GitHub releases page. |

The pipeline editor can also use other peer actions that declare compatible
Link input/output types and are installed, enabled, available on this platform
and pinned to the saved action version. An unavailable or changed action hides
the pipeline from `box.pipelines`; its saved card remains available for repair.
Wheel, Lens and Look consume the live pipeline offers through `box.pipelines`
and invoke them with `box.pipeline.run` and `options.pipeline`.

The standard `app.status` response includes Box's app-defined
`status.mode: "background" | "foreground"`, recording how this instance was
started. `--background` starts to tray with no window; activation does not
change the startup mode. Arcade Tools uses it when relaunching after updates.

## How requests run

- **Running Box**: requests go through the same job manager as the Island,
  so they appear under Background jobs, report progress, and can be
  cancelled (`job.cancel`, or the caller disconnecting).
- **Box not running**: headless tools run as a one-shot process
  (`arcade-desktop --arcade-invoke`), with no UI, tray or listener.
  `box.open` starts Box in the background first.
- **Delegated selection grant**: a path that arrives over the Link counts as
  user-selected for that job only. It is canonicalized and checked like a
  file-dialog selection (a regular, readable file), and the grant is revoked
  when the job ends.
- **Outputs** follow Box's rule: a new file, never an overwrite. Tools that
  write beside their input do so; inputs handed over in another app's
  temporary handoff folder are first copied into Box's own artifact folder,
  so the output never lands in a folder its creator deletes.

## Command line

```sh
arcade-box tools --json                      # tools, Link types and presets (no database; a few ms)
arcade-box run arcade.image.convert --file shot.png --preset webp
echo '{"action":"box:arcade.text.clean","preset":"clean","inputs":[{"type":"text/plain","text":" a "}]}' \
  | arcade-box run --stdin-json             # an Arcade Link invoke request, result as JSON
arcade-desktop --arcade-manifest             # Box's manifest (no side effects)
arcade-desktop --arcade-invoke < request     # one-shot mode
```

`arcade-box` is the canonical name of the CLI; `arcadebox` stays as an alias.
The CLI and the desktop app keep separate databases (see
[storage](storage.md)); one-shot requests from other apps use the desktop's.

## Settings

The Link switch (`link_enabled`) and the per-app toggles
(`link_disabled_peers`) live in the settings table. With the switch off,
Box's manifest lists no actions and nothing listens.

Settings includes **Connected apps**: a master switch, one row for Lens,
Look, Wheel, Clipboard, Shelf, Find and Tools with installed/running state and
a **Use with Arcade Box** toggle, Get links for missing apps, and
registry/endpoint diagnostics.
Get hands the selected app to `tools.install` (`options.app`) when Arcade
Tools is available; otherwise it opens that app's GitHub releases page.
Registry and endpoint changes are watched through OS notifications; no
polling runs while idle. Disk reads and probes run on worker threads.

The first action row in Island results offers **Preview** for files, **Send to my devices ↗** with a
payload preview and an explicit Send button, **Add to Wheel**, **Add to Shelf**
and **Pin** for a single image. Device sends over 16 MiB stay disabled with the standard
reason. Each request rechecks availability and limits before invoking the
owner; Private mode and Lens's safety checks remain in those apps. Missing
or disabled peers and unavailable actions contribute no result entries. Structured results are sent
as plain JSON text when the user chooses to send them.

Add to Shelf appears when Arcade Shelf is installed, enabled for Box and
reports `shelf.add` available, and the result has something Shelf can keep.
Output files travel together as one `file/any[]` batch of paths (Shelf types
each path itself) and folders as `folder/reference`; Box's artifact folder is
persistent, so Shelf's references stay valid. Text outputs go as `text/plain`,
`text/url` or `text/rich`; other text (for example HTML source) goes as plain
text. Structured-only results contribute no Add to Shelf entry. Shelf adds to
its active collection without opening a window and answers with a short
message, shown under the row.

Add to Wheel is also offered on tools, their named presets, and saved
pipeline cards. It uses `structured/arcade-action`; pipeline references
carry `action: "box.pipeline.run"` and `options: {pipeline: id}`. Interactive
screen/clipboard tools use `box.open` to open their existing Box UI. Wheel
confirms the slot in its own Settings. A Wheel version without structured
action support contributes no entry.

## Local peer providers

The OCR provider selector offers **Arcade Lens (local)** when the cached
registry reports `lens.recognize` available. It requests `ocrOnly: true` and
records the peer executable, version, provider ID and OCR engine in the result.
Automatic selection keeps Tesseract first on Linux and prefers Lens on Windows
and macOS. A specific Tesseract language pack still uses Tesseract.

Screen screenshot, QR and OCR actions select their region with `lens.capture`
when available. Ruler, pin and color hand over to `lens.capture_and_act` with
`mode: measure`, `pin` or `color`. Lens handles all three hints directly. Captured PNGs are validated, copied
into Box's normal artifact/grant boundary, and remain local. Without Lens, the
native platform picker and screen tools keep their existing behavior. Lens's
capture API returns still images and Lens exposes no recording action, so
screen recording keeps Box's own platform recorder. Windows/macOS recording is
built and tested in CI but has not been run interactively.

Clipboard history opens `clipboard.pick` when available; selection does not
change the system clipboard. Without it, Box's own history view stays intact.
The picker and peer capture calls run on workers, can be cancelled, and refresh
from registry change events. Shortcut fields show **Used by <app name>** from
the cached registry, without contacting the peer.

## Verification

Run Box's integration checks through the isolated ecosystem runner:

```sh
E2E_VERBOSE=1 python3 ../Arcade-link/tools/e2e.py --only box
```

The normal embedded desktop can also be checked alone and with real peers,
including its shortcut, a tool run, startup mode and result-row visibility:

```sh
ARCADE_E2E_SHOTS=../Arcade-link/.orch/shots python3 ../Arcade-link/tools/e2e.py run -- python3 scripts/verify-desktop-isolated.py
ARCADE_E2E_SHOTS=../Arcade-link/.orch/shots python3 ../Arcade-link/tools/e2e.py run -- python3 scripts/verify-desktop-isolated.py --peers
```

These checks use `target/release/arcade-desktop`; `--binary` can select the
ordinary debug build. The script refuses to run outside the isolated runner.

The checks exercise a resident preset with `job.progress` and `job.done`,
one-shot progress without a listener or manifest, new outputs without
overwriting the selected input, cancellation without partial outputs,
an unavailable engine's reason, and `box.open` showing the Island. The core
tests verify that a delegated file grant is revoked after the job and on
input preparation failure. Real cross-app checks also drive Lens selection into
resize 50% → WebP → Clipboard history, exercise remembered effect approval,
cancellation and a peer crash, and repair a changed action version in the editor.
Pure provider-path tests cover WinGet/scoop/Program Files and Homebrew/app bundles
on Linux. Runtime checks use a private D-Bus session,
Xvfb, and temporary HOME/XDG/Arcade directories.

## Platforms

| | Linux X11 | Linux Wayland | Windows | macOS |
|---|---|---|---|---|
| Exposed actions, one-shot mode | tested | tested (headless) | CI-built and tested; not run interactively | CI-built and tested; not run interactively |
| Lens/Clipboard delegation and shortcut warnings | tested under Xvfb | not run | CI-built; not run interactively | CI-built; not run interactively |
