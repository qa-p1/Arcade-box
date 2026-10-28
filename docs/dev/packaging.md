# Packaging and release manifests

Installer configuration targets NSIS, AppImage and dmg. Installer builds merge
`apps/desktop/src-tauri/tauri.bundle.conf.json`, which embeds `arcade-box` and
`arcade-plugin-worker` using Tauri's [external binary support](https://v2.tauri.app/develop/sidecar/).
The CLI and worker live beside the desktop executable (AppImage `usr/bin`,
Windows installation directory, macOS `Arcade Box.app/Contents/MacOS`).
Ordinary cargo builds use the base configuration and require no staged sidecars.
Both build paths embed the production frontend through the default
`custom-protocol` feature.

From the repository root, on the native target machine:

```sh
export CARGO_BUILD_JOBS=3
npm ci --prefix apps/desktop/frontend
npm install --prefix target/tauri-cli --cache target/npm-cache --no-audit --no-fund @tauri-apps/cli@2.12.1
python3 scripts/prepare-bundle.py
cd apps/desktop/src-tauri
node ../../../target/tauri-cli/node_modules/@tauri-apps/cli/tauri.js build --config tauri.bundle.conf.json --bundles appimage
```

Choose `nsis` on Windows or `dmg` on macOS instead of `appimage`. The Python
staging script builds the canonical CLI and trusted plugin worker with three
Cargo jobs, reads the native Rust target triple, and copies target-suffixed
sidecars into the ignored `binaries` directory. It accepts `--target` for an
installed Rust target; cross-platform toolchains are not provided by this repo.
No WASM fixture rebuild, installation, app launch or startup-file edit is needed.

macOS CI builds a universal DMG for Apple Silicon and Intel. The desktop app,
CLI and trusted worker all contain both architectures. On a Mac, add the
`aarch64-apple-darwin` and `x86_64-apple-darwin` Rust targets, stage with
`python3 scripts/prepare-bundle.py --target universal-apple-darwin`, then pass
`--target universal-apple-darwin --bundles dmg` to the Tauri command above.
The staging script combines the CLI and worker with `lipo` before bundling.
The DMG smoke check verifies all three binaries' slices, installs into a
temporary home, and checks receipt, bundle and CLI removal after uninstall.

CI checks out Box; Cargo resolves Arcade Link from the pinned `v0.3.0` git tag.
It runs Rust/frontend checks, builds installers, and uploads one artifact
per platform. Linux, Windows and macOS jobs passed on 2026-10-08; interactive
Windows/macOS behavior is still unverified (see [status](../STATUS.md)).
The macOS adapter minimum is 14 for screen selection; recording output requires
15. The NSIS installation is per user. Linux CI uses Ubuntu 24.04; an AppImage
built on a newer system does not prove compatibility with older glibc versions.

## Stable and nightly

Releases start by hand, the same way in every Arcade repo:

1. Run **Tag a release** (`tag.yml`) with the tag (`v0.4.0`) and the full SHA
   of a commit on `main`. It checks that the commit is on `main` and that
   `Cargo.toml`'s `[workspace.package] version` matches the tag, then pushes
   an annotated tag.
2. Run **Release** (`release.yml`) on that tag with the `stable` channel. Run it
   on `main` with `nightly` to refresh the rolling `nightly` pre-release.

`release.yml` calls `ci.yml`, so the installers it publishes are the ones CI
built and installed, repaired and uninstalled on each OS. It then checks the
three installer kinds, writes `arcade-release.json` and `SHA256SUMS.txt`, and
publishes. Asset names are unchanged from earlier releases (spaces become
`-`; nightly files carry `nightly` in place of the version). Tags are never
pushed by hand.

The generator is vendored unchanged from Arcade Link `v0.3.0`, commit
`33650a8` (see [VENDORED](../../VENDORED.md)):

```sh
python3 scripts/arcade-release.py --id arcade.box --version 0.3.5 --channel stable \
  --notes https://github.com/qa-p1/Arcade-box/releases/tag/v0.3.5 dist/
(cd dist && sha256sum -c SHA256SUMS.txt)
```

The schema-1 manifest includes the app ID, version, channel, Link protocol 1,
release-notes URL, and each installer's OS, architecture, kind, byte size and
SHA-256. NSIS silent installation uses `/S`. The checksum file covers every
input asset; the generated manifest and checksum file are excluded from their
own checksum input list. Local validation covers stable/nightly classification
and checksum verification using fake NSIS/AppImage/dmg assets. It does not
publish them or claim native installer execution.

## Local validation result

The native CLI/worker staging and production Tauri desktop build completed on
Linux. The AppImage bundler then timed out downloading
`https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64`
(`timeout: global`), so no local AppImage was produced. This download failure is
a limitation of that local attempt. Native CI subsequently built all three
installer formats successfully; fake installer manifests and checksums also
passed for both channels. Ordinary debug/release desktop builds are rebuilt
without the bundling overlay before the isolated standalone GUI checks.
