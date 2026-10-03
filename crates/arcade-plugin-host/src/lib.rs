//! Isolated Component Model plugin installation and execution.
//!
//! Components receive the versioned Arcade capability interface in
//! `sdk/wit/arcade-tool/1.0.0/world.wit`. WASI Preview 2 standard shims are
//! available with an empty, deny-by-default context: there are no preopened
//! directories, inherited process handles, or allowed network addresses.

mod fingerprint;
mod install;
mod manifest;
#[cfg(feature = "runtime")]
mod runtime;
#[cfg(feature = "runtime")]
pub mod worker;
mod worker_protocol;

pub use fingerprint::modified_time_ns;
pub use install::{InstallApproval, InstallError, InstalledPlugin, PluginInstaller};
pub use manifest::{
    API_VERSION, AccessScope, COMPONENT_FILE, FilesystemPermissions, MANIFEST_FILE, ManifestError,
    NetworkMode, NetworkPermission, PackageMetadata, PermissionGrant, PluginManifest,
    PluginPermissions, PluginRuntimeConfig, sha256_component,
};
#[cfg(feature = "runtime")]
pub use runtime::{GrantedInput, PluginHost, PluginHostConfig, PluginHostError};
#[cfg(feature = "runtime")]
pub use worker::execute_invocation;
pub use worker_protocol::{
    WORKER_PROTOCOL_VERSION, WorkerInvocation, WorkerOutcome, WorkerResponse, WorkerSelectedInput,
};
