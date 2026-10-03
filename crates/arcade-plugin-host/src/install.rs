use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::manifest::{
    COMPONENT_FILE, MANIFEST_FILE, ManifestError, PermissionGrant, PluginManifest,
    max_component_bytes, sha256_component,
};

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const STATE_FILE: &str = "installed.json";

#[derive(Debug, Clone, Default)]
pub struct InstallApproval {
    /// Permissions explicitly granted during this install or update.
    pub grants: BTreeSet<PermissionGrant>,
    /// Required to add permissions beyond the currently installed grant set.
    pub acknowledge_escalation: bool,
}

#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    pub(crate) manifest: PluginManifest,
    pub(crate) package_dir: PathBuf,
    pub(crate) component_path: PathBuf,
    pub(crate) grants: BTreeSet<PermissionGrant>,
}

impl InstalledPlugin {
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn package_dir(&self) -> &Path {
        &self.package_dir
    }

    pub fn component_path(&self) -> &Path {
        &self.component_path
    }

    pub fn grants(&self) -> &BTreeSet<PermissionGrant> {
        &self.grants
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallationState {
    plugin_id: String,
    active_version: String,
    component_sha256: String,
    grants: BTreeSet<PermissionGrant>,
}

pub struct PluginInstaller;

impl PluginInstaller {
    /// Validate an unpacked package and return its manifest without installing
    /// it. Desktop permission preview uses this before asking for approval.
    pub fn inspect_local_package(source_dir: &Path) -> Result<PluginManifest, InstallError> {
        let source_dir = fs::canonicalize(source_dir)?;
        let manifest_path = regular_file(&source_dir.join(MANIFEST_FILE))?;
        let component_path = regular_file(&source_dir.join(COMPONENT_FILE))?;
        let manifest_bytes = read_bounded(&manifest_path, MAX_MANIFEST_BYTES)?;
        let manifest: PluginManifest = serde_json::from_slice(&manifest_bytes)?;
        manifest.validate()?;
        validate_plugin_id(&manifest.tool_manifest.id)?;
        let component_bytes = read_bounded(&component_path, max_component_bytes() as u64)?;
        if sha256_component(&component_bytes) != manifest.package.component_sha256 {
            return Err(InstallError::ComponentHashMismatch);
        }
        Ok(manifest)
    }

    /// Validate and copy an unpacked local plugin package into the private
    /// application data directory. The source package must contain only the
    /// fixed names `plugin.json` and `component.wasm`.
    pub fn install_local_package(
        source_dir: &Path,
        install_root: &Path,
        approval: InstallApproval,
    ) -> Result<InstalledPlugin, InstallError> {
        let source_dir = fs::canonicalize(source_dir)?;
        let manifest_path = regular_file(&source_dir.join(MANIFEST_FILE))?;
        let component_path = regular_file(&source_dir.join(COMPONENT_FILE))?;
        let manifest_bytes = read_bounded(&manifest_path, MAX_MANIFEST_BYTES)?;
        let manifest: PluginManifest = serde_json::from_slice(&manifest_bytes)?;
        manifest.validate()?;
        validate_plugin_id(&manifest.tool_manifest.id)?;

        let component_bytes = read_bounded(&component_path, max_component_bytes() as u64)?;
        let actual_hash = sha256_component(&component_bytes);
        if actual_hash != manifest.package.component_sha256 {
            return Err(InstallError::ComponentHashMismatch);
        }

        let requested = manifest.permissions()?.requested_grants();
        if !approval.grants.is_subset(&requested) {
            return Err(InstallError::UnexpectedGrant);
        }

        let root = ensure_private_dir(install_root)?;
        let plugin_dir = root.join(&manifest.tool_manifest.id);
        ensure_safe_child(&root, &plugin_dir)?;
        ensure_private_dir(&plugin_dir)?;
        let plugin_dir = fs::canonicalize(&plugin_dir)?;
        if !plugin_dir.starts_with(&root) {
            return Err(InstallError::UnsafePath);
        }

        let existing = read_state(&plugin_dir)?;
        let granted = resolve_grants(&manifest, existing.as_ref(), &approval)?;
        let incoming_version = Version::parse(&manifest.tool_manifest.version)
            .map_err(|_| InstallError::InvalidVersion(manifest.tool_manifest.version.clone()))?;
        if let Some(state) = &existing {
            let current_version = Version::parse(&state.active_version)
                .map_err(|_| InstallError::CorruptInstallation)?;
            match incoming_version.cmp(&current_version) {
                std::cmp::Ordering::Less => return Err(InstallError::Downgrade),
                std::cmp::Ordering::Equal if state.component_sha256 != actual_hash => {
                    return Err(InstallError::VersionReused);
                }
                _ => {}
            }
        }

        let versions_dir = plugin_dir.join("versions");
        ensure_safe_child(&plugin_dir, &versions_dir)?;
        ensure_private_dir(&versions_dir)?;
        let target_dir = versions_dir.join(incoming_version.to_string());
        ensure_safe_child(&versions_dir, &target_dir)?;
        if target_dir.exists() {
            let existing_component = regular_file(&target_dir.join(COMPONENT_FILE))?;
            let installed_bytes = read_bounded(&existing_component, max_component_bytes() as u64)?;
            if sha256_component(&installed_bytes) != actual_hash {
                return Err(InstallError::VersionReused);
            }
        } else {
            let staging = tempfile::Builder::new()
                .prefix(".staging-")
                .tempdir_in(&versions_dir)?;
            private_write(&staging.path().join(MANIFEST_FILE), &manifest_bytes)?;
            private_write(&staging.path().join(COMPONENT_FILE), &component_bytes)?;
            let staging_path = staging.keep();
            if let Err(error) = fs::rename(&staging_path, &target_dir) {
                let _ = fs::remove_dir_all(&staging_path);
                return Err(InstallError::Io(error));
            }
        }

        let state = InstallationState {
            plugin_id: manifest.tool_manifest.id.clone(),
            active_version: incoming_version.to_string(),
            component_sha256: actual_hash,
            grants: granted.clone(),
        };
        write_state_atomically(&plugin_dir, &state)?;

        Ok(InstalledPlugin {
            component_path: target_dir.join(COMPONENT_FILE),
            package_dir: target_dir,
            manifest,
            grants: granted,
        })
    }

    pub fn load_installed(
        install_root: &Path,
        plugin_id: &str,
    ) -> Result<InstalledPlugin, InstallError> {
        validate_plugin_id(plugin_id)?;
        let root = fs::canonicalize(install_root)?;
        let plugin_dir = root.join(plugin_id);
        ensure_safe_child(&root, &plugin_dir)?;
        let plugin_dir = fs::canonicalize(plugin_dir)?;
        if !plugin_dir.starts_with(&root) {
            return Err(InstallError::UnsafePath);
        }
        let state = read_state(&plugin_dir)?.ok_or(InstallError::NotInstalled)?;
        if state.plugin_id != plugin_id {
            return Err(InstallError::CorruptInstallation);
        }
        let version_dir = plugin_dir.join("versions").join(&state.active_version);
        ensure_safe_child(&plugin_dir.join("versions"), &version_dir)?;
        let manifest_path = regular_file(&version_dir.join(MANIFEST_FILE))?;
        let component_path = regular_file(&version_dir.join(COMPONENT_FILE))?;
        let manifest: PluginManifest =
            serde_json::from_slice(&read_bounded(&manifest_path, MAX_MANIFEST_BYTES)?)?;
        manifest.validate()?;
        let component_bytes = read_bounded(&component_path, max_component_bytes() as u64)?;
        let component_hash = sha256_component(&component_bytes);
        if component_hash != state.component_sha256
            || component_hash != manifest.package.component_sha256
        {
            return Err(InstallError::ComponentHashMismatch);
        }
        if !manifest
            .permissions()?
            .requested_grants()
            .is_subset(&state.grants)
        {
            return Err(InstallError::CorruptInstallation);
        }
        Ok(InstalledPlugin {
            manifest,
            package_dir: version_dir,
            component_path,
            grants: state.grants,
        })
    }

    /// Load and verify each plugin installed beneath the private root.
    pub fn list_installed(install_root: &Path) -> Result<Vec<InstalledPlugin>, InstallError> {
        if !install_root.exists() {
            return Ok(Vec::new());
        }
        let root = fs::canonicalize(install_root)?;
        let metadata = fs::symlink_metadata(&root)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(InstallError::UnsafePath);
        }
        let mut plugins = Vec::new();
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() || !file_type.is_dir() {
                return Err(InstallError::UnsafePath);
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            validate_plugin_id(&id)?;
            plugins.push(Self::load_installed(&root, &id)?);
        }
        plugins.sort_by(|left, right| {
            left.manifest
                .tool_manifest
                .id
                .cmp(&right.manifest.tool_manifest.id)
        });
        Ok(plugins)
    }

    /// Remove one validated plugin installation. The install root and plugin
    /// directory are canonicalized and checked before recursive removal.
    pub fn uninstall(install_root: &Path, plugin_id: &str) -> Result<(), InstallError> {
        validate_plugin_id(plugin_id)?;
        let root = fs::canonicalize(install_root)?;
        let plugin_dir = root.join(plugin_id);
        ensure_safe_child(&root, &plugin_dir)?;
        let plugin_dir = fs::canonicalize(&plugin_dir)?;
        if !plugin_dir.starts_with(&root) {
            return Err(InstallError::UnsafePath);
        }
        let metadata = fs::symlink_metadata(&plugin_dir)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(InstallError::UnsafePath);
        }
        if read_state(&plugin_dir)?.is_none() {
            return Err(InstallError::NotInstalled);
        }
        fs::remove_dir_all(plugin_dir)?;
        Ok(())
    }
}

fn resolve_grants(
    manifest: &PluginManifest,
    existing: Option<&InstallationState>,
    approval: &InstallApproval,
) -> Result<BTreeSet<PermissionGrant>, InstallError> {
    let requested = manifest.permissions()?.requested_grants();
    let previous = existing.map(|state| &state.grants);
    let escalation: BTreeSet<_> = requested
        .iter()
        .filter(|grant| !previous.is_some_and(|grants| grants.contains(grant)))
        .copied()
        .collect();
    if !escalation.is_empty() && existing.is_some() && !approval.acknowledge_escalation {
        return Err(InstallError::PermissionEscalation(escalation));
    }
    let granted = previous
        .into_iter()
        .flat_map(|grants| grants.iter().copied())
        .chain(approval.grants.iter().copied())
        .collect::<BTreeSet<_>>();
    let missing: BTreeSet<_> = requested.difference(&granted).copied().collect();
    if !missing.is_empty() {
        return Err(InstallError::PermissionGrantRequired(missing));
    }
    Ok(requested)
}

fn read_state(plugin_dir: &Path) -> Result<Option<InstallationState>, InstallError> {
    let path = plugin_dir.join(STATE_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let path = regular_file(&path)?;
    let bytes = read_bounded(&path, MAX_MANIFEST_BYTES)?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}

fn write_state_atomically(
    plugin_dir: &Path,
    state: &InstallationState,
) -> Result<(), InstallError> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_path = plugin_dir.join(format!(".installed-{nonce}.tmp"));
    let bytes = serde_json::to_vec_pretty(state)?;
    private_write(&temp_path, &bytes)?;
    let state_path = plugin_dir.join(STATE_FILE);
    if state_path.exists() {
        regular_file(&state_path)?;
    }
    fs::rename(temp_path, state_path)?;
    Ok(())
}

fn validate_plugin_id(id: &str) -> Result<(), InstallError> {
    if !id.starts_with("arcade.")
        || id.split('.').count() < 3
        || !id.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '.'
                || character == '-'
        })
        || Path::new(id)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(InstallError::InvalidId(id.into()));
    }
    Ok(())
}

fn ensure_safe_child(parent: &Path, child: &Path) -> Result<(), InstallError> {
    let child_name = child.file_name().ok_or(InstallError::UnsafePath)?;
    if child_name.is_empty() {
        return Err(InstallError::UnsafePath);
    }
    let child_parent = child.parent().ok_or(InstallError::UnsafePath)?;
    let canonical_parent = fs::canonicalize(parent)?;
    let canonical_child_parent = fs::canonicalize(child_parent)?;
    if canonical_parent != canonical_child_parent {
        return Err(InstallError::UnsafePath);
    }
    if let Ok(metadata) = fs::symlink_metadata(child)
        && metadata.file_type().is_symlink()
    {
        return Err(InstallError::UnsafePath);
    }
    Ok(())
}

fn ensure_private_dir(path: &Path) -> Result<PathBuf, InstallError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(InstallError::UnsafePath);
        }
    } else {
        fs::create_dir_all(path)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(fs::canonicalize(path)?)
}

fn regular_file(path: &Path) -> Result<PathBuf, InstallError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(InstallError::UnsafePath);
    }
    Ok(fs::canonicalize(path)?)
}

fn read_bounded(path: &Path, max_bytes: u64) -> Result<Vec<u8>, InstallError> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > max_bytes {
        return Err(InstallError::PackageTooLarge);
    }
    let file = File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(InstallError::PackageTooLarge);
    }
    Ok(bytes)
}

fn private_write(path: &Path, bytes: &[u8]) -> Result<(), InstallError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("I/O error during plugin installation: {0}")]
    Io(#[from] io::Error),
    #[error("invalid plugin JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("invalid plugin ID {0}")]
    InvalidId(String),
    #[error("plugin package exceeds the size limit")]
    PackageTooLarge,
    #[error("plugin component hash does not match its manifest")]
    ComponentHashMismatch,
    #[error("plugin installation path is not a safe regular directory/file")]
    UnsafePath,
    #[error("invalid semantic version {0}")]
    InvalidVersion(String),
    #[error("plugin downgrade requires a separate explicit rollback flow")]
    Downgrade,
    #[error("a plugin version cannot be reused with different component bytes")]
    VersionReused,
    #[error("installation state is corrupt")]
    CorruptInstallation,
    #[error("plugin requests permissions that must be acknowledged: {0:?}")]
    PermissionGrantRequired(BTreeSet<PermissionGrant>),
    #[error("plugin update escalates permissions and requires acknowledgement: {0:?}")]
    PermissionEscalation(BTreeSet<PermissionGrant>),
    #[error("install approval contains a permission the plugin did not request")]
    UnexpectedGrant,
    #[error("plugin is not installed")]
    NotInstalled,
}
