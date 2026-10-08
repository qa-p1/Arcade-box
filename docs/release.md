# Release and supply chain

## Current implementation

Native CI builds and tests on Linux, Windows and macOS, then produces AppImage,
NSIS and dmg artifacts with the CLI and trusted plugin worker. The release
workflow publishes `arcade-release.json` and `SHA256SUMS.txt` from successful
`main` CI. See [packaging](packaging.md) for commands and triggers and
[status](STATUS.md) for the dated verification record.

The ecosystem implementation remains on `arcade/link`; merging to `main` may
publish an application release. Compilation and packaging do not establish
interactive behavior on Windows/macOS or every Wayland compositor.

## Release gates still outstanding

Publisher signing/notarization, a signed in-app updater, signed plugin registry,
revocation and a complete release SBOM/provenance policy are not implemented by
this work. Current checksum verification over HTTPS checks integrity, not
publisher signatures. Do not describe unsigned builds as signed or verified by
a publisher key.

The workspace declares GPL-3.0-or-later provisionally. The repository currently
has no main application LICENSE text; final license approval, that text, and a
packaged dependency/license review are release gates. The checked-in locked
license inventory passes CI but is not a substitute for that review. See
[ADR 0005](adr/0005-license-policy.md) and [license inventory](../licenses/README.md).

Before publication, run the applicable standalone and integration checks,
review platform limits, verify checksums against the actual packaged files,
and complete the remaining release gates. Preserve user settings and keep
credentials scoped to the release jobs that require them.
