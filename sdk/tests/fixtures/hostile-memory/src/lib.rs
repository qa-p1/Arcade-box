wit_bindgen::generate!({
    path: "../../../wit/arcade-tool/1.0.0",
    world: "arcade-tool",
});

struct HostileMemory;

impl Guest for HostileMemory {
    fn run(_request: ToolRequest) -> Result<ToolResult, ToolError> {
        let bytes = vec![0x41; 16 * 1024 * 1024];
        core::hint::black_box(&bytes);
        Ok(ToolResult {
            outputs: vec![arcade::tool::types::ToolValue {
                kind: arcade::tool::types::ValueKind::Text,
                value: format!("allocated {} bytes", bytes.len()),
                mime: "text/plain".into(),
            }],
            warnings: Vec::new(),
            metadata_json: "{}".into(),
        })
    }
}

export!(HostileMemory);
