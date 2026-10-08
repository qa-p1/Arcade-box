# Security threat model

## Assets and trust boundaries

Assets include user files, decrypted material, clipboard text, credentials, provider binaries, plugin packages, job artifacts, and update signing keys. Trust boundaries are the UI-to-core command facade, core-to-plugin runtime, core-to-provider subprocess, registry/download channel, platform APIs, and temporary storage. User input, filenames, archives, webpages, QR contents, deep links, plugins, and discovered executables are untrusted.

The table distinguishes required security policy from current implementation; the status section below identifies unfinished release controls.

| Threat | Required controls |
|---|---|
| Malicious plugin | Wasmtime Component Model sandbox, explicit Arcade host capability, no raw Tauri bridge, memory/fuel/deadline limits. WASI Preview 2 receives an empty context with no preopened filesystem, inherited process handles, or permitted network addresses. Verify private-file and outbound-socket denial with adversarial components. |
| Compromised registry or package | Signed packages, cryptographic hashes, pinned provenance, compatibility checks, user-visible author/license/permissions, rollback, and no permission escalation without acknowledgement. |
| Malicious archive / traversal / unsafe symlink | Canonical destination containment, reject traversal and unsafe links, private staging, decompression limits, collision policy, and adversarial fixtures. |
| Hostile filename / path | Treat names as data, no shell interpolation, canonicalize scoped paths, reject path traversal and reserved/invalid targets, preview destructive rename plans. |
| Hostile webpage or redirect | Network tool permissions, redirect and size limits, no automatic opening of suspicious destinations, isolate any browser renderer, do not expose local filesystem to page scripts. |
| Malicious QR/deep link | Decode locally and display raw target before action. Parse app URI scheme as untrusted structured input; allow only known tool/pipeline IDs and validated typed parameters, never arbitrary commands. |
| PATH hijacking / fake provider executable | Resolve absolute path, inspect identity/provenance, bounded safe version and capability probes, user override, structured argv, sanitized environment, and visible selected path. |
| Symlink race / writes outside grant | Bind operations to opened handles or canonical scoped roots, avoid check-then-open races, revalidate before writes, and confine archive extraction/output. |
| Partial download replacement | Write to a private temporary target, verify supplied checksum/signature where available, atomically move after validation, and never overwrite source implicitly. |
| Update compromise | Signed update metadata and binaries, key protection/rotation/revocation, reject unsigned artifacts, publish checksums/provenance/SBOM. |
| Credential leakage | OS credential store; redact logs and history; no default token upload; never persist API keys in SQLite or expose cookies to plugins. The Groq key is read from the environment or a git-ignored `.env`, passed to curl through a private `0600` header file deleted after the call, and never placed in argv, logs, or results. |
| Temporary artifact leakage | Private permissions, scoped per-job directories, cleanup on completion/cancel/crash, ownership checks before recovery cleanup, retention policy for explicit outputs. |
| Clipboard privacy abuse | Clipboard history off by default; invocation-only context by default; expiry, exclusions, sensitive-content suppression, clear-all, no raw clipboard logging. |
| Process execution abuse | Allowlisted provider capability adapters, absolute verified executable, structured argument arrays, controlled environment/workdir, bounded output, timeout and cancellation. |

## Enforcement rule

Manifest declarations are requests, not enforcement. The host checks every operation against the effective grant and OS capability. If the platform cannot enforce a requested boundary, do not claim the permission was enforced; explain the limitation or reject the operation. Security controls require negative tests: denied network, unrelated-file reads, archive traversal, invalid manifests, and update tampering.

## Current status

The core executes installed plugins through a trusted worker process, with hash/grant checks, opaque selected-input references, bounded execution and process cancellation. The host tests cover denied unrelated-file reads, WASI network/filesystem denial, permission escalation, and fuel/deadline limits. The worker has a separate 30-second process timeout covering compilation and execution; compilation is not an OS memory sandbox.

Provider execution and output publication use the current scoped process/artifact helpers. There is no archive-tool family in the shipped catalog. Signed application/plugin update metadata, registry revocation and a general managed-model installer are not implemented. Existing release assets use SHA-256 checksums; Arcade Tools validates these over HTTPS, which is not publisher authentication. See [release policy and current coverage](../release.md) and [plugins](../plugin-model.md).
