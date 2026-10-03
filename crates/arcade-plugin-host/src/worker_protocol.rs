use std::path::PathBuf;

use arcade_contract::{ToolRequest, ToolResult};
use serde::{Deserialize, Serialize};

pub const WORKER_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerInvocation {
    pub protocol_version: u32,
    pub install_root: PathBuf,
    pub plugin_id: String,
    pub request: ToolRequest,
    #[serde(default)]
    pub selected_inputs: Vec<WorkerSelectedInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerSelectedInput {
    pub index: u32,
    /// A path minted by the trusted parent after the user selected the file.
    /// This field is consumed only by the worker and is never copied to WIT.
    pub path: PathBuf,
    pub expected_size: u64,
    /// Signed decimal nanoseconds since the Unix epoch. A string avoids
    /// truncation on platforms whose timestamp range exceeds JSON integers.
    pub expected_modified_ns: String,
    #[serde(default)]
    pub expected_device: Option<u64>,
    #[serde(default)]
    pub expected_inode: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerResponse {
    pub protocol_version: u32,
    pub outcome: WorkerOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkerOutcome {
    Completed { result: ToolResult },
    Rejected { code: String, message: String },
}
