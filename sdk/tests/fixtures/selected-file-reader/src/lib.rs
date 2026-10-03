wit_bindgen::generate!({
    path: "../../../wit/arcade-tool/1.0.0",
    world: "arcade-tool",
});

use arcade::tool::{capabilities::read_selected_input, types::ValueKind};

struct SelectedFileReader;

impl Guest for SelectedFileReader {
    fn run(request: ToolRequest) -> Result<ToolResult, ToolError> {
        let input = request
            .inputs
            .first()
            .ok_or_else(|| ToolError {
                code: "missing-input".into(),
                message: "Select one file first.".into(),
            })?;
        if input.kind != ValueKind::File {
            return Err(ToolError {
                code: "unsupported-input".into(),
                message: "This fixture accepts a selected file only.".into(),
            });
        }
        let bytes = read_selected_input(0, 0, 32).map_err(|_| ToolError {
            code: "read-denied".into(),
            message: "The host denied file access.".into(),
        })?;
        let output = String::from_utf8_lossy(&bytes).into_owned();
        Ok(ToolResult {
            outputs: vec![arcade::tool::types::ToolValue {
                kind: ValueKind::Text,
                value: format!("{}:{output}", input.value),
                mime: "text/plain".into(),
            }],
            warnings: Vec::new(),
            metadata_json: "{}".into(),
        })
    }
}

export!(SelectedFileReader);
