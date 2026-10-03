# Plugin model

The public tool contract is shared by first-party tools and community plugins. First-party tools must use the same typed I/O, permission checks, provider capabilities, result model, and version rules; internal Rust code is not a second privileged plugin API.

## Component Model ABI

The v1 interface is [`sdk/wit/arcade-tool/1.0.0/world.wit`](../sdk/wit/arcade-tool/1.0.0/world.wit), package `arcade:tool@1.0.0`. It takes a `tool-request` with plural `inputs` and returns typed outputs, warnings, and JSON metadata. The Rust ABI is generated from WIT; Rust struct layout is not the compatibility boundary. `apiVersion` is a string and must equal `1`; the package schema version is currently `1`.

The host uses Wasmtime `48.0.2` and the Component Model. The plugin host crate is kept out of default workspace builds because Wasmtime is a substantial optional dependency and should initialize only when a plugin is used. Guest components are built for `wasm32-wasip2` with `wit-bindgen` 0.57.1. See the external sample under [`sdk/examples/community-uppercase`](../sdk/examples/community-uppercase).

## Host capabilities

The host links the declared Arcade capability interface and WASI Preview 2 standard shims. WASI receives an empty context: there are no preopened directories, inherited arguments/environment/stdio, or allowed socket addresses; TCP, UDP, and IP name lookup are disabled. There is no process-spawn capability. Tests execute components that try to read `/etc/passwd` and open a TCP connection; both operations fail.

User-selected file reads are available only through `read-selected-input(input-index, offset, max-bytes)`. The trusted host supplies an already-open handle for a selected request input. The guest sees an opaque `input:<index>` token, never a path, and can read no more than 64 KiB per call. A permission declaration alone does not create a grant: installation records a separate user approval, and the host checks it again for every call. Writes and plugin network access are not supported by host API v1 and requests for them are rejected. File/artifact outputs are also rejected until a scoped output capability is available.

Default limits are 64 MiB total linear memory per component store, one linear memory, 25 million fuel units, a two-second execution deadline, at most 1,024 input/output items, and 1 MiB serialized request/result text. Adversarial fixtures exercise the memory, fuel, and deadline limits. The runtime serializes calls per engine because epoch interruption is engine-wide. Wasmtime component compilation is not included in the execution deadline; package size and integrity checks bound the input, and compilation isolation is a follow-up hardening item before accepting arbitrary registry packages.

## Packages and installation

An unpacked local package must contain:

```text
plugin.json
component.wasm
```

`plugin.json` wraps a normal Arcade tool manifest in `toolManifest` and adds `schemaVersion` plus package author, source, license, and `componentSha256`. The installer rejects unsupported schema/API versions, a non-WASM execution declaration, unimplemented tools, unsupported permissions/output types, malformed IDs/versions, hash mismatches, and unexpected grants. It copies the package to private versioned directories. Reusing a version with different component bytes and downgrades are rejected.

An update cannot gain a permission silently. New requested grants require both an explicit grant and `acknowledgeEscalation`; approval to install a package is separate from signature/authenticity. Local developer installs are hash-verified but not author-authenticated. Registry signatures, revocation, rollback UI, disable/uninstall UI, and permission explanation UI are still pending.

## Developer flow

Build the sample and adversarial fixtures with `scripts/build-plugin-fixtures.sh`. The script uses a process-local Cargo target directory under the repository's ignored `target/` folder and emits a sample package under `crates/arcade-plugin-host/tests/fixtures/community-uppercase/`. It requires Rust 1.95 or newer and the `wasm32-wasip2` target. The sample uses only the published WIT contract and `wit-bindgen`; the host test installs it through the local-package API, executes it, and verifies its real result.

The intended complete SDK flow remains `arcadebox sdk new`, `arcadebox sdk dev`, validate, run fixtures, and package. Developer mode should report manifest errors, effective permissions, logs, typed inputs/outputs, and host/runtime compatibility.

## Registry and trust

The official registry should be an index of signed/versioned packages, not an execution authority. The client must verify signatures and package hashes, check API compatibility, display author/source/license/permissions, and block silent permission escalation. Signing-key rotation and revocation procedures belong in release and registry operations docs. A valid signature identifies a publisher; it does not mean plugin code is trustworthy.

## Current state

The versioned WIT ABI, standalone Wasmtime host, local hash-checked installer, selected-file read capability, empty-context WASI policy, sample component, and network/filesystem/fuel/deadline tests are implemented. The host is not yet added to the root workspace or connected to the core registry/runtime. Permission UI, signed registry packages, plugin updates/removal UI, sandboxed advanced views, output-file capability, and release integration remain planned.
