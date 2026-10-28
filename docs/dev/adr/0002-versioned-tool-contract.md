# ADR 0002: Versioned serialized tool contract

- **Status:** Accepted.
- **Problem:** First-party tools, plugins, pipelines, GUI, and CLI must agree on inputs and outputs without binding the ecosystem to Rust struct layout.
- **Decision:** Use a versioned JSON-compatible manifest and typed request/result contract. Give tools stable IDs and semantic versions; version API compatibility independently. File/media values are scoped references, not inline payloads.
- **Reason:** Stable serialized contracts support external SDKs, persistence, and safe pipeline validation while keeping large data off IPC.
- **Consequences:** Schema validation and compatibility tests are required. Breaking changes require an explicit API transition; old pipeline references must be detected and migrated or blocked clearly.
