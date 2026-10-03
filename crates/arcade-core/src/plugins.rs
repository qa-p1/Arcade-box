//! Local plugin registry and short-lived worker dispatch.
//!
//! Wasmtime is intentionally initialized only by `arcade-plugin-worker`.
//! Core reads and validates installed manifests, then sends one bounded JSON
//! request to the adjacent trusted worker executable.

use std::{
    collections::BTreeSet,
    fs::{self, File},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

use arcade_contract::{ResultStatus, ToolManifest, ToolRequest, ToolResult, ValueKind};
use arcade_plugin_host::{
    InstallApproval, PermissionGrant, PluginInstaller, PluginManifest, WORKER_PROTOCOL_VERSION,
    WorkerInvocation, WorkerOutcome, WorkerResponse, WorkerSelectedInput, modified_time_ns,
};
use serde::{Deserialize, Serialize};

use crate::{grants::FileGrants, process};

const WORKER_TIMEOUT: Duration = Duration::from_secs(30);
const WORKER_OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSummary {
    pub manifest: PluginManifest,
    pub granted_permissions: BTreeSet<PermissionGrant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPreview {
    pub manifest: PluginManifest,
    pub installed_version: Option<String>,
    pub requested_permissions: BTreeSet<PermissionGrant>,
    pub additional_permissions: BTreeSet<PermissionGrant>,
}

pub struct PluginRuntime {
    install_root: PathBuf,
    worker_executable: Option<PathBuf>,
    first_party_ids: BTreeSet<String>,
}

impl PluginRuntime {
    pub fn new(install_root: PathBuf, first_party_ids: BTreeSet<String>) -> Self {
        Self {
            install_root,
            worker_executable: adjacent_worker_executable(),
            first_party_ids,
        }
    }

    /// Construct a runtime with a known worker path for packaging and tests.
    /// The worker is still opened by absolute path and never through a shell.
    pub fn with_worker(
        install_root: PathBuf,
        first_party_ids: BTreeSet<String>,
        worker_executable: PathBuf,
    ) -> Self {
        Self {
            install_root,
            worker_executable: Some(worker_executable),
            first_party_ids,
        }
    }

    pub fn install_root(&self) -> &Path {
        &self.install_root
    }

    pub fn installed_tools(&self) -> Result<Vec<ToolManifest>, String> {
        Ok(PluginInstaller::list_installed(&self.install_root)
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|plugin| plugin.manifest().tool_manifest.clone())
            .collect())
    }

    pub fn list(&self) -> Result<Vec<PluginSummary>, String> {
        PluginInstaller::list_installed(&self.install_root)
            .map(|plugins| {
                plugins
                    .into_iter()
                    .map(|plugin| PluginSummary {
                        manifest: plugin.manifest().clone(),
                        granted_permissions: plugin.grants().clone(),
                    })
                    .collect()
            })
            .map_err(|error| error.to_string())
    }

    pub fn preview(&self, source_dir: &Path) -> Result<PluginPreview, String> {
        let manifest = PluginInstaller::inspect_local_package(source_dir)
            .map_err(|error| error.to_string())?;
        self.ensure_non_conflicting(&manifest.tool_manifest.id)?;
        let requested_permissions = manifest
            .permissions()
            .map_err(|error| error.to_string())?
            .requested_grants();
        let installed = PluginInstaller::list_installed(&self.install_root)
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|plugin| plugin.manifest().tool_manifest.id == manifest.tool_manifest.id);
        let installed_version = installed
            .as_ref()
            .map(|plugin| plugin.manifest().tool_manifest.version.clone());
        let previous_grants = installed
            .map(|plugin| plugin.grants().clone())
            .unwrap_or_default();
        let additional_permissions = requested_permissions
            .difference(&previous_grants)
            .copied()
            .collect();
        Ok(PluginPreview {
            manifest,
            installed_version,
            requested_permissions,
            additional_permissions,
        })
    }

    pub fn install(
        &self,
        source_dir: &Path,
        approval: InstallApproval,
    ) -> Result<PluginSummary, String> {
        let manifest = PluginInstaller::inspect_local_package(source_dir)
            .map_err(|error| error.to_string())?;
        self.ensure_non_conflicting(&manifest.tool_manifest.id)?;
        let installed =
            PluginInstaller::install_local_package(source_dir, &self.install_root, approval)
                .map_err(|error| error.to_string())?;
        Ok(PluginSummary {
            manifest: installed.manifest().clone(),
            granted_permissions: installed.grants().clone(),
        })
    }

    pub fn uninstall(&self, plugin_id: &str) -> Result<(), String> {
        self.ensure_non_conflicting(plugin_id)?;
        PluginInstaller::uninstall(&self.install_root, plugin_id).map_err(|error| error.to_string())
    }

    pub fn run_dev(
        &self,
        source_dir: &Path,
        request: &ToolRequest,
        approval: InstallApproval,
        grants: &FileGrants,
        cancelled: &AtomicBool,
    ) -> Result<ToolResult, String> {
        let manifest = PluginInstaller::inspect_local_package(source_dir)
            .map_err(|error| error.to_string())?;
        self.ensure_non_conflicting(&manifest.tool_manifest.id)?;
        let temporary_root = tempfile::tempdir().map_err(|error| error.to_string())?;
        PluginInstaller::install_local_package(source_dir, temporary_root.path(), approval)
            .map_err(|error| error.to_string())?;
        let temporary_runtime = PluginRuntime {
            install_root: temporary_root.path().to_path_buf(),
            worker_executable: self.worker_executable.clone(),
            first_party_ids: self.first_party_ids.clone(),
        };
        temporary_runtime.execute(request, grants, cancelled)
    }

    pub fn execute(
        &self,
        request: &ToolRequest,
        grants: &FileGrants,
        cancelled: &AtomicBool,
    ) -> Result<ToolResult, String> {
        let plugin = PluginInstaller::load_installed(&self.install_root, &request.tool_id)
            .map_err(|error| error.to_string())?;
        let permissions = plugin
            .manifest()
            .permissions()
            .map_err(|error| error.to_string())?;
        let worker = self
            .worker_executable
            .as_ref()
            .ok_or("Arcade Box plugin worker executable is not installed")?;
        validate_worker_executable(worker)?;

        let mut held_handles: Vec<File> = Vec::new();
        let mut selected_inputs = Vec::new();
        if permissions.permits_selected_file_read()
            && plugin
                .grants()
                .contains(&PermissionGrant::ReadUserSelectedFiles)
        {
            for (index, input) in request.inputs.iter().enumerate() {
                if !matches!(input.kind, ValueKind::File | ValueKind::Artifact) {
                    continue;
                }
                if input.kind != ValueKind::Artifact {
                    return Err("Plugin file inputs must come from Arcade Box's file picker".into());
                }
                grants
                    .verify_type(&input.value, &input.mime)
                    .map_err(|error| error.to_string())?;
                let handle = grants
                    .open_scoped(&input.value)
                    .map_err(|error| error.to_string())?;
                let metadata = handle.metadata().map_err(|error| error.to_string())?;
                if !metadata.is_file() {
                    return Err("Selected plugin input is no longer a regular file".into());
                }
                let path = grants
                    .resolve(&input.value)
                    .map_err(|error| error.to_string())?;
                #[cfg(unix)]
                let (expected_device, expected_inode) = {
                    use std::os::unix::fs::MetadataExt;
                    (Some(metadata.dev()), Some(metadata.ino()))
                };
                #[cfg(not(unix))]
                let (expected_device, expected_inode) = (None, None);
                selected_inputs.push(WorkerSelectedInput {
                    index: index as u32,
                    path,
                    expected_size: metadata.len(),
                    expected_modified_ns: modified_time_ns(&metadata)
                        .map_err(|error| error.to_string())?,
                    expected_device,
                    expected_inode,
                });
                // Keep the original grant handle alive throughout worker
                // execution. The worker opens its own handle and verifies its
                // identity against this fingerprint before exposing it to WIT.
                held_handles.push(handle);
            }
        }

        let invocation = WorkerInvocation {
            protocol_version: WORKER_PROTOCOL_VERSION,
            install_root: self.install_root.clone(),
            plugin_id: request.tool_id.clone(),
            request: request.clone(),
            selected_inputs,
        };
        let input = serde_json::to_vec(&invocation).map_err(|error| error.to_string())?;
        let spec = process::ProcessSpec {
            executable: worker.clone(),
            args: Vec::new(),
            current_dir: Some(self.install_root.clone()),
            timeout: WORKER_TIMEOUT,
            output_limit: WORKER_OUTPUT_LIMIT,
        };
        let output = process::run_with_input(&spec, cancelled, &input)
            .map_err(|error| format!("Plugin worker failed: {error}"))?;
        drop(held_handles);
        if !output.status.success() {
            return Err(format!(
                "Plugin worker exited unsuccessfully: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        let response: WorkerResponse = serde_json::from_slice(&output.stdout)
            .map_err(|_| "Plugin worker returned an invalid response".to_string())?;
        if response.protocol_version != WORKER_PROTOCOL_VERSION {
            return Err("Plugin worker returned an unsupported protocol version".into());
        }
        match response.outcome {
            WorkerOutcome::Completed { result } => Ok(result),
            WorkerOutcome::Rejected { code, message } => Err(format!(
                "Plugin worker rejected request ({code}): {message}"
            )),
        }
    }

    fn ensure_non_conflicting(&self, plugin_id: &str) -> Result<(), String> {
        if self.first_party_ids.contains(plugin_id) || plugin_id.starts_with("arcade.pipeline.") {
            return Err(format!(
                "Plugin ID {plugin_id} conflicts with a reserved Arcade Box tool ID"
            ));
        }
        Ok(())
    }
}

fn adjacent_worker_executable() -> Option<PathBuf> {
    let current_exe = std::env::current_exe().ok()?;
    let file_name = if cfg!(windows) {
        "arcade-plugin-worker.exe"
    } else {
        "arcade-plugin-worker"
    };
    let candidate = current_exe.parent()?.join(file_name);
    validate_worker_executable(&candidate).ok()?;
    fs::canonicalize(candidate).ok()
}

fn validate_worker_executable(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("Plugin worker path must be absolute".into());
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| "Arcade Box plugin worker executable was not found".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Plugin worker path must be a regular non-symlink executable".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err("Plugin worker file is not executable".into());
        }
    }
    Ok(())
}

pub fn result_from_worker_error(tool_id: &str, error: impl Into<String>) -> ToolResult {
    ToolResult {
        tool_id: tool_id.to_owned(),
        status: ResultStatus::Error,
        outputs: Vec::new(),
        message: Some(error.into()),
        warnings: Vec::new(),
        metadata: Default::default(),
    }
}
