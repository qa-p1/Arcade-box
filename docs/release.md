# Release and supply chain

Core application updates must use the desktop framework's signed update mechanism. Downloaded binaries are verified before install; an unsigned or invalid package is rejected. Plugins and managed providers have separate hashes/signatures and versioning. A registry cannot silently widen plugin permissions.

Release candidates build on actual Windows, macOS, and Linux runners. Package targets include Windows x64 and ARM64 where support is verified; macOS Apple Silicon and an Intel/universal strategy based on build tests; and Linux packages selected from tested AppImage, deb, rpm, and Flatpak flows. Production macOS releases require signing and notarization. Linux Wayland support requires a real mainstream desktop run, not just compilation.

Every release includes checksums, signatures, changelog, SBOM where practical, third-party license notices, source revision, and build provenance. Reproducible builds are pursued where toolchains allow. Release workflows should keep secrets scoped and protect signing credentials. Contributions require formatting, linting, unit/integration, security and license checks, plus cross-platform compile gates; functional desktop jobs run at an appropriate cadence.

Do not call a release ready until the acceptance flows for Island, provider reuse, context, pipeline, community plugin, malicious plugin denial, large media, and Wayland have passed and the tool readiness matrix contains no fake completion claims.
