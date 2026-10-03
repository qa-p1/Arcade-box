#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target/plugin-fixtures}"
fixture_dir="$repo_root/crates/arcade-plugin-host/tests/fixtures"

build_component() {
  local manifest_path="$1"
  local artifact_name="$2"
  local output_name="$3"
  CARGO_TARGET_DIR="$target_dir" cargo build \
    --manifest-path "$repo_root/$manifest_path/Cargo.toml" \
    --target wasm32-wasip2 --release --lib
  install -D "$target_dir/wasm32-wasip2/release/$artifact_name.wasm" \
    "$fixture_dir/$output_name"
}

build_component sdk/examples/community-uppercase arcade_community_uppercase community-uppercase/component.wasm
build_component sdk/tests/fixtures/hostile-filesystem arcade_fixture_hostile_filesystem hostile-filesystem.wasm
build_component sdk/tests/fixtures/hostile-network arcade_fixture_hostile_network hostile-network.wasm
build_component sdk/tests/fixtures/selected-file-reader arcade_fixture_selected_file_reader selected-file-reader.wasm
build_component sdk/tests/fixtures/hostile-spin arcade_fixture_hostile_spin hostile-spin.wasm
build_component sdk/tests/fixtures/hostile-memory arcade_fixture_hostile_memory hostile-memory.wasm

python3 - "$fixture_dir/community-uppercase" <<'PY'
import hashlib
import json
import pathlib
import sys

package_dir = pathlib.Path(sys.argv[1])
component = (package_dir / "component.wasm").read_bytes()
manifest = {
    "schemaVersion": 1,
    "toolManifest": {
        "id": "arcade.community.uppercase",
        "version": "0.1.0",
        "apiVersion": "1",
        "name": "Uppercase Text (Example Plugin)",
        "description": "Convert one or more text inputs to uppercase.",
        "category": "text",
        "aliases": ["uppercase text", "to uppercase"],
        "privacyClass": "LOCAL",
        "inputs": ["text/plain"],
        "outputs": ["text/plain"],
        "providers": [],
        "status": "implemented",
        "platforms": {"windows": "supported", "macos": "supported", "wayland": "supported", "x11": "supported"},
        "permissions": {"filesystem": {"read": "none", "write": "none"}, "network": {"mode": "none", "domains": []}},
        "execution": {"runtime": "wasm"},
        "phrases": ["capitalize text", "uppercase selected text"],
        "relatedTools": [],
    },
    "package": {
        "author": "Arcade Box SDK Examples",
        "source": "https://github.com/arcade-box/arcade-box/tree/main/sdk/examples/community-uppercase",
        "license": "MIT",
        "componentSha256": "sha256:" + hashlib.sha256(component).hexdigest(),
    },
}
(package_dir / "plugin.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
PY

printf 'Built public sample and adversarial components in %s\n' "$fixture_dir"
