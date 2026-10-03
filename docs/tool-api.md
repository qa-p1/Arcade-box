# Tool API, version 1

The serialized manifest and request/result types are the compatibility boundary. Rust struct layout is not an ABI. [`catalog/tools.json`](../catalog/tools.json) demonstrates the manifest fields; [`catalog/tools.schema.json`](../catalog/tools.schema.json) validates its JSON shape. The shared Rust definitions live in `arcade-contract`.

## Manifest

Required metadata includes a stable `arcade.*` ID, semantic tool `version`, `apiVersion`, name, description, category, aliases, privacy class, typed inputs and outputs, provider capabilities, permissions, execution runtime, implementation status, and platform status. Optional `phrases` and `relatedTools` enrich search and recommendations. Unknown fields should be retained or ignored compatibly; do not give unknown fields authority.

An ID is never reused for a different action. Breaking serialized contract changes require a new API version. Tool behavior/options can evolve under semantic versioning. The application version, tool API version, tool version, provider version, and plugin package version are separate.

The optional `ui` field is a versioned standard form definition. `input` declares a text, URL, single-file, multiple-file, or no-input surface, with item limits and ordering where useful. `controls` define host-rendered text, number, select, and toggle options. Selects list valid choices; numeric fields can bound values; `showWhen` can reveal fields based on an earlier control. The same descriptor is available to first-party and external tools. The host validates it before displaying a tool, and the frontend turns numeric fields into numeric JSON request options. Rich previews can sit beside the standard form without changing request/output contracts.

A select choice may list `requires`: provider capabilities in `kind:name` form (`encoder:libx265`, `mux:mov`, `filter:hflip`). The host disables a choice whose capabilities no compatible provider reports and labels it *not installed*. This lets one catalog entry offer optional codecs and formats without failing at run time. The runtime still checks the same capabilities before it runs anything, so a stale or hand-written request gets a clear error instead of a provider failure.

A tool whose input is `files` and whose `inputs` type is an array (for example `file/video[]`) can process each selected file on its own with the same options. This is how Video Convert, Crop, Compress, and Extract Audio handle batches of up to 50 files. A per-file failure becomes a result warning, and the other files continue. Pipelines accept a single-item output (`file/video`) where a matching array input (`file/video[]`) is expected.

## Values and I/O

Input and output types are MIME-like strings such as `text/plain`, `text/url`, `structured/json`, `file/pdf`, `file/image[]`, `screen/region`, and `clipboard/image`. `[]` denotes a batch/collection shape. Tools should declare specific types and accept documented compatible subtypes where safe.

`ToolRequest.inputs` is plural so a tool can accept batches and multi-file work without special request shapes. `ToolValue` carries a kind, value/reference, and MIME type. File and artifact values must be scoped references, not inline bytes or caller-controlled arbitrary paths. The host verifies user selection, canonicalizes the target, and checks that operations remain within the granted scope. Large objects stay outside IPC and SQLite.

Plugin API v1 is defined by [`sdk/wit/arcade-tool/1.0.0/world.wit`](../sdk/wit/arcade-tool/1.0.0/world.wit). For WASM components, user-selected file values cross the component boundary only as opaque input-index tokens; a scoped host call reads a bounded chunk from an already-open handle. The host does not pass source paths to the plugin. File writes, network requests, and file/artifact outputs are rejected by the current plugin host until their scoped capabilities are implemented.

## Execution and results

The host validates a request against the manifest before dispatch. `execution.runtime` identifies a host implementation such as `builtin` or the Wasmtime component host. A result includes tool ID, success/error status, typed outputs, a user-facing message, warnings, and structured metadata. Progress and cancellation are job events, not fake success results. Errors retain technical diagnostics for an expandable detail view while exposing a clear next action.

## Permission and privacy contract

Permissions describe the maximum operation the host can grant: user-selected filesystem read/write, invoke-only clipboard, user-initiated screen capture, scoped system access, or network. A manifest is a request, not authority. The runtime intersects it with the user's grant and host policy. Updates that add or widen access require acknowledgement.

Privacy classes are `LOCAL`, `NETWORK`, and `CLOUD`. Effective disclosure follows the selected provider at execution time. No tool may silently upload content because a cloud provider is configured.

## Validation

Reject unsupported API versions, malformed or duplicate IDs, blank names/descriptions, missing output types, incompatible pipeline edges, and undeclared capabilities. Validate plugin schemas before install and again before execution. Preserve a copy of the manifest version used by saved pipelines so incompatible updates can be detected and migrated explicitly.
