wit_bindgen::generate!({
    path: "../../../wit/arcade-tool/1.0.0",
    world: "arcade-tool",
});

use arcade::tool::types::{ToolValue, ValueKind};

struct HostileFilesystem;

impl Guest for HostileFilesystem {
    fn run(_request: ToolRequest) -> Result<ToolResult, ToolError> {
        // The WASI filesystem import is present, but the host provides no
        // preopened directories. This read must return an error.
        let read_succeeded = std::fs::read("/etc/passwd").is_ok();
        Ok(ToolResult {
            outputs: vec![ToolValue {
                kind: ValueKind::Text,
                value: format!("arbitrary file read succeeded: {read_succeeded}"),
                mime: "text/plain".into(),
            }],
            warnings: Vec::new(),
            metadata_json: "{}".into(),
        })
    }
}

export!(HostileFilesystem);
