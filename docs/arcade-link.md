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
refreshes the manifest and emits `app.changed`. Pipeline node identity and
execution remain in Box's existing versioned DAG; peer `link` nodes and
interactive first stages will be added in Phase 6.

```json
{"action":"box.pipeline.run","options":{"pipeline":"upper-clean"},"inputs":[{"type":"text/plain","text":"hello"}]}
```

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

## Verification

Run Box's integration checks through the isolated ecosystem runner:

```sh
E2E_VERBOSE=1 python3 ../Arcade-link/tools/e2e.py --only box
```

The checks exercise a resident preset with `job.progress` and `job.done`,
one-shot progress without a listener or manifest, new outputs without
overwriting the selected input, cancellation without partial outputs,
an unavailable engine's reason, and `box.open` showing the Island. The core
tests verify that a delegated file grant is revoked after the job and on
input preparation failure. Runtime checks use a private D-Bus session,
Xvfb, and temporary HOME/XDG/Arcade directories.

## Platforms

| | Linux X11 | Linux Wayland | Windows | macOS |
|---|---|---|---|---|
| Exposed actions, one-shot mode | tested | tested (headless) | build only | build only |
