# Community uppercase plugin

This example is intentionally outside Arcade Box's first-party tool source. It
uses only the public WIT contract in `sdk/wit/arcade-tool/1.0.0` and the
`wit-bindgen` guest SDK. It accepts any number of text inputs and returns one
uppercase text output for each input.

Build and package it from the repository root with:

```sh
scripts/build-plugin-fixtures.sh
```

The script requires Rust with the `wasm32-wasip2` target. It writes the
component and a hash-complete `plugin.json` into
`crates/arcade-plugin-host/tests/fixtures/community-uppercase/`. The resulting
directory is an unpacked local package accepted by `PluginInstaller`.

The plugin requests no filesystem or network capability. Source file paths are
never part of its request; file and artifact values are represented by opaque
input handles in the host ABI.
