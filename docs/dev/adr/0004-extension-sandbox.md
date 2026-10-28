# ADR 0004: No in-process native plugins

- **Status:** Accepted; standalone Wasmtime host implemented, root runtime integration pending.
- **Problem:** Third-party native libraries loaded into the application process can bypass permission boundaries and destabilize the whole application.
- **Decision:** Do not load arbitrary native plugin code in the core process. Use Wasmtime 48.0.2 with the WebAssembly Component Model and versioned WIT API. Components receive an explicit Arcade host capability. WASI Preview 2 shims run with an empty context, no filesystem preopens, no inherited process handles, and no permitted network addresses. Advanced UI runs in an isolated view without a raw desktop bridge.
- **Reason:** The public extension API needs an enforceable boundary and a stable portable contract.
- **Consequences:** Rust 1.95 is the minimum supported toolchain for Wasmtime 48.0.2. Plugin execution has memory, fuel, and wall-clock limits; user-selected file access uses an already-open scoped handle and bounded reads. Network, filesystem writes, and plugin file outputs are rejected until dedicated capabilities and permission UI exist. Native providers require a separately governed trust model.
