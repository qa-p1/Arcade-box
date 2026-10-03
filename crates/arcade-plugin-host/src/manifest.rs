use std::collections::BTreeSet;

use arcade_contract::{ImplementationStatus, ToolManifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const API_VERSION: &str = "1";
pub const COMPONENT_FILE: &str = "component.wasm";
pub const MANIFEST_FILE: &str = "plugin.json";
const MAX_COMPONENT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginManifest {
    pub schema_version: u32,
    pub tool_manifest: ToolManifest,
    pub package: PackageMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackageMetadata {
    pub author: String,
    pub source: String,
    pub license: String,
    pub component_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccessScope {
    None,
    UserSelected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilesystemPermissions {
    pub read: AccessScope,
    pub write: AccessScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkMode {
    None,
    Domains,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkPermission {
    pub mode: NetworkMode,
    #[serde(default)]
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginPermissions {
    pub filesystem: FilesystemPermissions,
    pub network: NetworkPermission,
}

impl PluginPermissions {
    pub fn none() -> Self {
        Self {
            filesystem: FilesystemPermissions {
                read: AccessScope::None,
                write: AccessScope::None,
            },
            network: NetworkPermission {
                mode: NetworkMode::None,
                domains: Vec::new(),
            },
        }
    }

    pub fn read_selected_only() -> Self {
        Self {
            filesystem: FilesystemPermissions {
                read: AccessScope::UserSelected,
                write: AccessScope::None,
            },
            network: NetworkPermission {
                mode: NetworkMode::None,
                domains: Vec::new(),
            },
        }
    }

    pub fn validate_supported(&self) -> Result<(), ManifestError> {
        if self.filesystem.write != AccessScope::None {
            return Err(ManifestError::UnsupportedPermission(
                "filesystem.write=user-selected".into(),
            ));
        }
        if self.network.mode != NetworkMode::None || !self.network.domains.is_empty() {
            return Err(ManifestError::UnsupportedPermission(
                "network access is not available to this host runtime".into(),
            ));
        }
        Ok(())
    }

    pub fn requested_grants(&self) -> BTreeSet<PermissionGrant> {
        let mut grants = BTreeSet::new();
        if self.filesystem.read == AccessScope::UserSelected {
            grants.insert(PermissionGrant::ReadUserSelectedFiles);
        }
        grants
    }

    pub fn permits_selected_file_read(&self) -> bool {
        self.filesystem.read == AccessScope::UserSelected
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionGrant {
    ReadUserSelectedFiles,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginRuntimeConfig {
    pub runtime: String,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.schema_version != 1 {
            return Err(ManifestError::SchemaVersion(self.schema_version));
        }
        if self.tool_manifest.api_version != API_VERSION {
            return Err(ManifestError::ApiVersion(
                self.tool_manifest.api_version.clone(),
            ));
        }
        self.tool_manifest
            .validate()
            .map_err(|error| ManifestError::ToolManifest(error.to_string()))?;
        if self
            .tool_manifest
            .outputs
            .iter()
            .any(|output| output.starts_with("file/") || output.starts_with("artifact/"))
        {
            return Err(ManifestError::UnsupportedOutput);
        }
        if self.tool_manifest.status != ImplementationStatus::Implemented {
            return Err(ManifestError::NotImplemented);
        }
        if self
            .tool_manifest
            .execution
            .get("runtime")
            .and_then(|v| v.as_str())
            != Some("wasm")
        {
            return Err(ManifestError::WrongRuntime);
        }
        let permissions: PluginPermissions =
            serde_json::from_value(self.tool_manifest.permissions.clone())
                .map_err(|error| ManifestError::Permissions(error.to_string()))?;
        permissions.validate_supported()?;
        if self.package.author.trim().is_empty()
            || self.package.source.trim().is_empty()
            || self.package.license.trim().is_empty()
        {
            return Err(ManifestError::MissingPackageMetadata);
        }
        if !self
            .package
            .component_sha256
            .strip_prefix("sha256:")
            .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(ManifestError::ComponentHash);
        }
        Ok(())
    }

    pub fn permissions(&self) -> Result<PluginPermissions, ManifestError> {
        serde_json::from_value(self.tool_manifest.permissions.clone())
            .map_err(|error| ManifestError::Permissions(error.to_string()))
    }
}

pub fn sha256_component(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    format!("sha256:{}", hex_bytes(&hash))
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(crate) fn max_component_bytes() -> usize {
    MAX_COMPONENT_BYTES
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("unsupported plugin manifest schema version {0}")]
    SchemaVersion(u32),
    #[error("unsupported plugin API version {0}")]
    ApiVersion(String),
    #[error("invalid tool manifest: {0}")]
    ToolManifest(String),
    #[error("plugin must provide an implemented tool")]
    NotImplemented,
    #[error("plugin execution runtime must be `wasm`")]
    WrongRuntime,
    #[error("invalid permissions: {0}")]
    Permissions(String),
    #[error("unsupported plugin permission: {0}")]
    UnsupportedPermission(String),
    #[error("package author, source, and license are required")]
    MissingPackageMetadata,
    #[error("component hash must be a sha256 digest")]
    ComponentHash,
    #[error("plugin component exceeds the 64 MiB package limit")]
    ComponentTooLarge,
    #[error(
        "file and artifact outputs are not supported until a scoped write capability is available"
    )]
    UnsupportedOutput,
}
