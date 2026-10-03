wit_bindgen::generate!({
    path: "../../wit/arcade-tool/1.0.0",
    world: "arcade-tool",
});

use arcade::tool::types::{ToolValue, ValueKind};

struct Uppercase;

impl Guest for Uppercase {
    fn run(request: ToolRequest) -> Result<ToolResult, ToolError> {
        let mut outputs = Vec::new();
        for input in request.inputs {
            if input.kind != ValueKind::Text {
                return Err(ToolError {
                    code: "unsupported-input".into(),
                    message: "This example accepts text inputs only.".into(),
                });
            }
            outputs.push(ToolValue {
                kind: ValueKind::Text,
                value: input.value.to_uppercase(),
                mime: "text/plain".into(),
            });
        }
        Ok(ToolResult {
            outputs,
            warnings: Vec::new(),
            metadata_json: "{}".into(),
        })
    }
}

export!(Uppercase);
