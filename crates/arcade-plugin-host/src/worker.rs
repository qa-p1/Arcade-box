//! Bounded stdin/stdout protocol used by the trusted `arcade-plugin-worker`
//! process. The desktop/core process can hand plugin compilation and
//! execution to this short-lived process, then contain its lifetime.

use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};

use arcade_contract::{ResultStatus, ToolResult, ValueKind};

use crate::{
    PermissionGrant, PluginHost, PluginHostConfig, PluginInstaller, WorkerInvocation,
    WorkerOutcome, WorkerResponse, WorkerSelectedInput, fingerprint::modified_time_ns,
    manifest::AccessScope, worker_protocol::WORKER_PROTOCOL_VERSION,
};

pub const MAX_WORKER_INPUT_BYTES: usize = 2 * 1024 * 1024;
/// Host results are bounded to 1 MiB before JSON encoding. JSON escaping can
/// expand a control-heavy result several-fold, so the outer protocol allows
/// up to 8 MiB.
pub const MAX_WORKER_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

/// Decode, validate, install-state-check, fingerprint-check, and execute one
/// invocation. This function is shared by the actual worker binary and tests.
pub fn execute_invocation(bytes: &[u8]) -> WorkerResponse {
    let invocation = match parse_invocation(bytes) {
        Ok(invocation) => invocation,
        Err((code, message)) => return rejected(code, message),
    };
    match execute_validated(invocation) {
        Ok(result) => WorkerResponse {
            protocol_version: WORKER_PROTOCOL_VERSION,
            outcome: WorkerOutcome::Completed { result },
        },
        Err((code, message)) => rejected(code, message),
    }
}

/// Process one JSON object from stdin and emit exactly one bounded JSON line.
/// A caller-level deadline must contain stdin reads and Wasmtime compilation.
pub fn run_stdio() -> io::Result<()> {
    let mut input = Vec::with_capacity(64 * 1024);
    io::stdin()
        .lock()
        .take((MAX_WORKER_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut input)?;
    let response = execute_invocation(&input);
    let mut encoded = serde_json::to_vec(&response)?;
    if encoded.len().saturating_add(1) > MAX_WORKER_OUTPUT_BYTES {
        encoded = serde_json::to_vec(&rejected(
            "response-too-large",
            "Plugin worker response exceeded its output limit",
        ))?;
    }
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    stdout.write_all(&encoded)?;
    stdout.write_all(b"\n")?;
    stdout.flush()
}

fn parse_invocation(bytes: &[u8]) -> Result<WorkerInvocation, (String, String)> {
    if bytes.len() > MAX_WORKER_INPUT_BYTES {
        return Err((
            "invocation-too-large".into(),
            "Plugin worker invocation exceeded its input limit".into(),
        ));
    }
    let invocation: WorkerInvocation = serde_json::from_slice(bytes).map_err(|_| {
        (
            "invalid-invocation".into(),
            "Plugin worker expected one valid JSON invocation".into(),
        )
    })?;
    if invocation.protocol_version != WORKER_PROTOCOL_VERSION {
        return Err((
            "unsupported-protocol-version".into(),
            format!(
                "Worker protocol version {} is unsupported; expected {}",
                invocation.protocol_version, WORKER_PROTOCOL_VERSION
            ),
        ));
    }
    if invocation.request.tool_id != invocation.plugin_id {
        return Err((
            "tool-id-mismatch".into(),
            "Invocation tool ID does not match the requested plugin".into(),
        ));
    }
    if invocation.selected_inputs.len() > 1024 {
        return Err((
            "too-many-selected-inputs".into(),
            "Invocation contains too many selected file inputs".into(),
        ));
    }
    let mut indices = BTreeSet::new();
    for selected in &invocation.selected_inputs {
        if !indices.insert(selected.index) {
            return Err((
                "duplicate-selected-input".into(),
                "Invocation contains duplicate selected input indexes".into(),
            ));
        }
        if selected
            .expected_modified_ns
            .parse::<i128>()
            .ok()
            .is_none_or(|nanos| nanos.to_string() != selected.expected_modified_ns.as_str())
            || selected.expected_modified_ns.is_empty()
            || !selected.path.is_absolute()
            || selected.path.as_os_str().is_empty()
            || selected.expected_device.is_some() != selected.expected_inode.is_some()
        {
            return Err((
                "invalid-file-fingerprint".into(),
                "Selected file path or fingerprint is missing or malformed".into(),
            ));
        }
        let Some(input) = invocation.request.inputs.get(selected.index as usize) else {
            return Err((
                "invalid-selected-input".into(),
                "Selected file index does not refer to a request input".into(),
            ));
        };
        if !matches!(input.kind, ValueKind::File | ValueKind::Artifact) {
            return Err((
                "invalid-selected-input".into(),
                "Selected file index does not refer to a file or artifact input".into(),
            ));
        }
    }
    Ok(invocation)
}

fn execute_validated(invocation: WorkerInvocation) -> Result<ToolResult, (String, String)> {
    let plugin = PluginInstaller::load_installed(&invocation.install_root, &invocation.plugin_id)
        .map_err(|error| ("plugin-not-installed".into(), error.to_string()))?;
    let permissions = plugin
        .manifest()
        .permissions()
        .map_err(|error| ("invalid-installed-plugin".into(), error.to_string()))?;
    if !invocation.selected_inputs.is_empty()
        && permissions.filesystem.read != AccessScope::UserSelected
    {
        return Err((
            "permission-not-granted".into(),
            "Plugin does not have permission to read user-selected files".into(),
        ));
    }
    if permissions.filesystem.read == AccessScope::UserSelected
        && !plugin
            .grants()
            .contains(&PermissionGrant::ReadUserSelectedFiles)
    {
        return Err((
            "permission-not-granted".into(),
            "Plugin installation has no user-selected file read grant".into(),
        ));
    }
    let mut granted_inputs = Vec::with_capacity(invocation.selected_inputs.len());
    for selected in &invocation.selected_inputs {
        let file = open_and_verify_selected(selected)
            .map_err(|error| ("selected-file-changed".into(), error))?;
        granted_inputs.push(crate::GrantedInput::from_reader(selected.index, file));
    }
    let host = PluginHost::new(PluginHostConfig::default())
        .map_err(|error| ("runtime-initialization-failed".into(), error.to_string()))?;
    let request = invocation.request;
    let error_tool_id = request.tool_id.clone();
    match host.execute(&plugin, &request, granted_inputs) {
        Ok(result) => Ok(result),
        Err(error) => Ok(ToolResult {
            tool_id: error_tool_id,
            status: ResultStatus::Error,
            outputs: Vec::new(),
            message: Some(error.to_string()),
            warnings: Vec::new(),
            metadata: Default::default(),
        }),
    }
}

fn open_and_verify_selected(selected: &WorkerSelectedInput) -> Result<File, String> {
    let link_metadata = fs::symlink_metadata(&selected.path)
        .map_err(|_| "Could not inspect selected file".to_string())?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        return Err("Selected input is not a regular, non-symlink file".into());
    }
    let file = open_without_following_links(&selected.path)
        .map_err(|_| "Could not open selected file safely".to_string())?;
    let opened = file
        .metadata()
        .map_err(|_| "Could not inspect opened selected file".to_string())?;
    if !opened.is_file() || opened.file_type().is_symlink() {
        return Err("Opened selected input is not a regular file".into());
    }
    if opened.len() != selected.expected_size {
        return Err("Selected file size changed after selection".into());
    }
    let actual_modified_ns = modified_time_ns(&opened)
        .map_err(|_| "Could not verify selected file modification time".to_string())?;
    if actual_modified_ns != selected.expected_modified_ns {
        return Err("Selected file changed after selection".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Some(expected_device), Some(expected_inode)) =
            (selected.expected_device, selected.expected_inode)
        {
            if opened.dev() != expected_device || opened.ino() != expected_inode {
                return Err("Selected file identity changed after selection".into());
            }
        }
    }
    #[cfg(windows)]
    if selected.expected_device.is_some() || selected.expected_inode.is_some() {
        return Err("Windows file fingerprints must omit Unix identity fields".into());
    }
    Ok(file)
}

fn open_without_following_links(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Open a reparse point itself so handle metadata can reject it instead
        // of transparently following it to a different target.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path)
}

fn rejected(code: impl Into<String>, message: impl Into<String>) -> WorkerResponse {
    WorkerResponse {
        protocol_version: WORKER_PROTOCOL_VERSION,
        outcome: WorkerOutcome::Rejected {
            code: code.into(),
            message: message.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_invalid_or_oversized_protocol_envelopes() {
        let invalid = execute_invocation(b"not json");
        assert!(matches!(invalid.outcome, WorkerOutcome::Rejected { .. }));

        let oversized = execute_invocation(&vec![b' '; MAX_WORKER_INPUT_BYTES + 1]);
        assert!(matches!(oversized.outcome, WorkerOutcome::Rejected { .. }));

        let unsupported = execute_invocation(
            &serde_json::to_vec(&json!({
                "protocolVersion":2,
                "installRoot":"/plugins",
                "pluginId":"arcade.test.plugin",
                "request":{"toolId":"arcade.test.plugin","inputs":[],"options":{}},
                "selectedInputs":[]
            }))
            .unwrap(),
        );
        assert!(
            matches!(unsupported.outcome, WorkerOutcome::Rejected { code, .. } if code == "unsupported-protocol-version")
        );
    }
}
