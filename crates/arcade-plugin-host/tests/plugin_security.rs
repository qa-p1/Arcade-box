use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    time::Duration,
};

use arcade_contract::{ToolRequest, ToolValue, ValueKind};
use arcade_plugin_host::{
    InstallApproval, InstallError, PermissionGrant, PluginHost, PluginHostConfig, PluginHostError,
    PluginInstaller, sha256_component,
};
use serde_json::{Value, json};
use tempfile::TempDir;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[allow(clippy::too_many_arguments)]
fn make_package(
    temp: &TempDir,
    id: &str,
    version: &str,
    component_name: &str,
    input_types: &[&str],
    output_types: &[&str],
    permissions: Value,
    api_version: &str,
) -> PathBuf {
    let package_dir = temp.path().join(format!("package-{id}"));
    fs::create_dir_all(&package_dir).unwrap();
    let component = fs::read(fixtures().join(component_name)).unwrap();
    fs::write(package_dir.join("component.wasm"), &component).unwrap();
    let manifest = json!({
        "schemaVersion": 1,
        "toolManifest": {
            "id": id,
            "version": version,
            "apiVersion": api_version,
            "name": "Plugin test fixture",
            "description": "A test plugin component.",
            "category": "tests",
            "aliases": [],
            "privacyClass": "LOCAL",
            "inputs": input_types,
            "outputs": output_types,
            "providers": [],
            "status": "implemented",
            "platforms": {},
            "permissions": permissions,
            "execution": {"runtime": "wasm"},
            "phrases": [],
            "relatedTools": []
        },
        "package": {
            "author": "Arcade Box tests",
            "source": "local:test-fixture",
            "license": "MIT",
            "componentSha256": sha256_component(&component)
        }
    });
    fs::write(
        package_dir.join("plugin.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    package_dir
}

fn no_permissions() -> Value {
    json!({
        "filesystem": {"read": "none", "write": "none"},
        "network": {"mode": "none", "domains": []}
    })
}

fn selected_read_permission() -> Value {
    json!({
        "filesystem": {"read": "user-selected", "write": "none"},
        "network": {"mode": "none", "domains": []}
    })
}

fn text_request(id: &str, text: &str) -> ToolRequest {
    ToolRequest {
        tool_id: id.into(),
        inputs: vec![ToolValue::text(text, "text/plain")],
        options: json!({}),
    }
}

#[test]
fn external_sample_installs_and_executes_through_public_contract() {
    let temp = tempfile::tempdir().unwrap();
    let package = fixtures().join("community-uppercase");
    let install_root = temp.path().join("installed");
    let installed =
        PluginInstaller::install_local_package(&package, &install_root, InstallApproval::default())
            .unwrap();
    let loaded =
        PluginInstaller::load_installed(&install_root, "arcade.community.uppercase").unwrap();
    assert_eq!(
        installed.manifest().tool_manifest.id,
        loaded.manifest().tool_manifest.id
    );

    let host = PluginHost::new(PluginHostConfig::default()).unwrap();
    let result = host
        .execute(
            &loaded,
            &text_request("arcade.community.uppercase", "Arcade Box"),
            Vec::new(),
        )
        .unwrap();
    assert_eq!(result.outputs.len(), 1);
    assert_eq!(result.outputs[0].value, "ARCADE BOX");
}

#[test]
fn selected_file_read_is_scoped_to_the_passed_handle_and_path_is_opaque() {
    let temp = tempfile::tempdir().unwrap();
    let package = make_package(
        &temp,
        "arcade.test.selected-reader",
        "1.0.0",
        "selected-file-reader.wasm",
        &["file/test"],
        &["text/plain"],
        selected_read_permission(),
        "1",
    );
    let install_root = temp.path().join("installed");
    let installed = PluginInstaller::install_local_package(
        &package,
        &install_root,
        InstallApproval {
            grants: [PermissionGrant::ReadUserSelectedFiles]
                .into_iter()
                .collect(),
            acknowledge_escalation: false,
        },
    )
    .unwrap();
    let request = ToolRequest {
        tool_id: "arcade.test.selected-reader".into(),
        inputs: vec![ToolValue {
            kind: ValueKind::File,
            value: "/private/user/source.txt".into(),
            mime: "file/test".into(),
        }],
        options: json!({}),
    };
    let host = PluginHost::new(PluginHostConfig::default()).unwrap();
    let result = host
        .execute(
            &installed,
            &request,
            vec![arcade_plugin_host::GrantedInput::from_reader(
                0,
                Cursor::new(b"user-selected-bytes".to_vec()),
            )],
        )
        .unwrap();
    assert_eq!(result.outputs[0].value, "input:0:user-selected-bytes");
    assert!(!result.outputs[0].value.contains("/private/user/source.txt"));
}

#[test]
fn host_capability_denies_selected_file_reads_without_permission() {
    let temp = tempfile::tempdir().unwrap();
    let package = make_package(
        &temp,
        "arcade.test.denied-reader",
        "1.0.0",
        "selected-file-reader.wasm",
        &["file/test"],
        &["text/plain"],
        no_permissions(),
        "1",
    );
    let installed = PluginInstaller::install_local_package(
        &package,
        &temp.path().join("installed"),
        InstallApproval::default(),
    )
    .unwrap();
    let request = ToolRequest {
        tool_id: "arcade.test.denied-reader".into(),
        inputs: vec![ToolValue {
            kind: ValueKind::File,
            value: "host-file-reference".into(),
            mime: "file/test".into(),
        }],
        options: json!({}),
    };
    let host = PluginHost::new(PluginHostConfig::default()).unwrap();
    let error = host
        .execute(
            &installed,
            &request,
            vec![arcade_plugin_host::GrantedInput::from_reader(
                0,
                Cursor::new(b"must-not-be-read".to_vec()),
            )],
        )
        .unwrap_err();
    assert!(matches!(
        error,
        PluginHostError::PluginReported { ref code, .. } if code == "read-denied"
    ));
}

#[test]
fn wasi_filesystem_and_network_attempts_are_denied_by_empty_context() {
    for (component, success_phrase) in [
        ("hostile-filesystem.wasm", "arbitrary file read succeeded"),
        ("hostile-network.wasm", "outbound connection succeeded"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let id = if component.contains("filesystem") {
            "arcade.test.hostile-filesystem"
        } else {
            "arcade.test.hostile-network"
        };
        let package = make_package(
            &temp,
            id,
            "1.0.0",
            component,
            &["text/plain"],
            &["text/plain"],
            no_permissions(),
            "1",
        );
        let installed = PluginInstaller::install_local_package(
            &package,
            &temp.path().join("installed"),
            InstallApproval::default(),
        )
        .unwrap();
        let host = PluginHost::new(PluginHostConfig::default()).unwrap();
        let result = host
            .execute(&installed, &text_request(id, "test"), Vec::new())
            .unwrap();
        let message = &result.outputs[0].value;
        assert!(
            message.contains(success_phrase) && message.ends_with("false"),
            "expected {success_phrase} to be denied, got: {message}"
        );
    }
}

#[test]
fn manifest_rejects_unknown_api_and_unsupported_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let unknown_api = make_package(
        &temp,
        "arcade.test.unknown-api",
        "1.0.0",
        "community-uppercase/component.wasm",
        &["text/plain"],
        &["text/plain"],
        no_permissions(),
        "9000",
    );
    let error = PluginInstaller::install_local_package(
        &unknown_api,
        &temp.path().join("unknown-api-install"),
        InstallApproval::default(),
    )
    .unwrap_err();
    assert!(matches!(error, InstallError::Manifest(_)));

    let unknown_permission = make_package(
        &temp,
        "arcade.test.unknown-permission",
        "1.0.0",
        "community-uppercase/component.wasm",
        &["text/plain"],
        &["text/plain"],
        json!({
            "filesystem": {"read": "entire-home-directory", "write": "none"},
            "network": {"mode": "none", "domains": []}
        }),
        "1",
    );
    let error = PluginInstaller::install_local_package(
        &unknown_permission,
        &temp.path().join("unknown-permission-install"),
        InstallApproval::default(),
    )
    .unwrap_err();
    assert!(matches!(error, InstallError::Manifest(_)));

    let network = make_package(
        &temp,
        "arcade.test.network-request",
        "1.0.0",
        "community-uppercase/component.wasm",
        &["text/plain"],
        &["text/plain"],
        json!({
            "filesystem": {"read": "none", "write": "none"},
            "network": {"mode": "domains", "domains": ["example.com"]}
        }),
        "1",
    );
    let error = PluginInstaller::install_local_package(
        &network,
        &temp.path().join("network-install"),
        InstallApproval::default(),
    )
    .unwrap_err();
    assert!(matches!(error, InstallError::Manifest(_)));

    let file_output = make_package(
        &temp,
        "arcade.test.file-output",
        "1.0.0",
        "community-uppercase/component.wasm",
        &["text/plain"],
        &["file/test"],
        no_permissions(),
        "1",
    );
    let error = PluginInstaller::install_local_package(
        &file_output,
        &temp.path().join("file-output-install"),
        InstallApproval::default(),
    )
    .unwrap_err();
    assert!(matches!(error, InstallError::Manifest(_)));
}

#[test]
fn permission_escalation_requires_acknowledgement_and_grant() {
    let temp = tempfile::tempdir().unwrap();
    let install_root = temp.path().join("installed");
    let initial = make_package(
        &temp,
        "arcade.test.permission-update",
        "1.0.0",
        "community-uppercase/component.wasm",
        &["text/plain"],
        &["text/plain"],
        no_permissions(),
        "1",
    );
    PluginInstaller::install_local_package(&initial, &install_root, InstallApproval::default())
        .unwrap();
    let update = make_package(
        &temp,
        "arcade.test.permission-update",
        "1.1.0",
        "selected-file-reader.wasm",
        &["file/test"],
        &["text/plain"],
        selected_read_permission(),
        "1",
    );
    let error =
        PluginInstaller::install_local_package(&update, &install_root, InstallApproval::default())
            .unwrap_err();
    assert!(matches!(error, InstallError::PermissionEscalation(_)));

    let error = PluginInstaller::install_local_package(
        &update,
        &install_root,
        InstallApproval {
            grants: [PermissionGrant::ReadUserSelectedFiles]
                .into_iter()
                .collect(),
            acknowledge_escalation: false,
        },
    )
    .unwrap_err();
    assert!(matches!(error, InstallError::PermissionEscalation(_)));

    let installed = PluginInstaller::install_local_package(
        &update,
        &install_root,
        InstallApproval {
            grants: [PermissionGrant::ReadUserSelectedFiles]
                .into_iter()
                .collect(),
            acknowledge_escalation: true,
        },
    )
    .unwrap();
    assert!(
        installed
            .grants()
            .contains(&PermissionGrant::ReadUserSelectedFiles)
    );
}

#[test]
fn plugin_component_respects_fuel_and_wall_clock_limits() {
    let temp = tempfile::tempdir().unwrap();
    let package = make_package(
        &temp,
        "arcade.test.spin",
        "1.0.0",
        "hostile-spin.wasm",
        &["text/plain"],
        &["text/plain"],
        no_permissions(),
        "1",
    );
    let installed = PluginInstaller::install_local_package(
        &package,
        &temp.path().join("installed"),
        InstallApproval::default(),
    )
    .unwrap();

    let fuel_host = PluginHost::new(PluginHostConfig {
        fuel: 10_000,
        ..PluginHostConfig::default()
    })
    .unwrap();
    assert!(matches!(
        fuel_host.execute(
            &installed,
            &text_request("arcade.test.spin", "spin"),
            Vec::new()
        ),
        Err(PluginHostError::WasmtimeError(_))
    ));

    let deadline = Duration::from_millis(40);
    let timeout_host = PluginHost::new(PluginHostConfig {
        fuel: u64::MAX,
        timeout: deadline,
        ..PluginHostConfig::default()
    })
    .unwrap();
    assert!(matches!(
        timeout_host.execute(&installed, &text_request("arcade.test.spin", "spin"), Vec::new()),
        Err(PluginHostError::Timeout(actual)) if actual == deadline
    ));
}

#[test]
fn plugin_linear_memory_is_bounded_by_store_limit() {
    let temp = tempfile::tempdir().unwrap();
    let package = make_package(
        &temp,
        "arcade.test.memory",
        "1.0.0",
        "hostile-memory.wasm",
        &["text/plain"],
        &["text/plain"],
        no_permissions(),
        "1",
    );
    let installed = PluginInstaller::install_local_package(
        &package,
        &temp.path().join("installed"),
        InstallApproval::default(),
    )
    .unwrap();
    let host = PluginHost::new(PluginHostConfig {
        memory_bytes: 8 * 1024 * 1024,
        ..PluginHostConfig::default()
    })
    .unwrap();
    assert!(matches!(
        host.execute(
            &installed,
            &text_request("arcade.test.memory", "allocate"),
            Vec::new()
        ),
        Err(PluginHostError::WasmtimeError(_))
    ));
}

#[test]
fn duplicate_granted_input_handles_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let package = make_package(
        &temp,
        "arcade.test.duplicate-input",
        "1.0.0",
        "selected-file-reader.wasm",
        &["file/test"],
        &["text/plain"],
        selected_read_permission(),
        "1",
    );
    let installed = PluginInstaller::install_local_package(
        &package,
        &temp.path().join("installed"),
        InstallApproval {
            grants: [PermissionGrant::ReadUserSelectedFiles]
                .into_iter()
                .collect(),
            acknowledge_escalation: false,
        },
    )
    .unwrap();
    let request = ToolRequest {
        tool_id: "arcade.test.duplicate-input".into(),
        inputs: vec![ToolValue {
            kind: ValueKind::File,
            value: "input".into(),
            mime: "file/test".into(),
        }],
        options: json!({}),
    };
    let error = PluginHost::new(PluginHostConfig::default())
        .unwrap()
        .execute(
            &installed,
            &request,
            vec![
                arcade_plugin_host::GrantedInput::from_reader(0, Cursor::new(Vec::new())),
                arcade_plugin_host::GrantedInput::from_reader(0, Cursor::new(Vec::new())),
            ],
        )
        .unwrap_err();
    assert!(matches!(error, PluginHostError::InvalidGrantedInput));
}

#[test]
fn source_fixture_package_exists_outside_the_host_crate() {
    let external_example =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sdk/examples/community-uppercase");
    assert!(external_example.join("src/lib.rs").is_file());
}
