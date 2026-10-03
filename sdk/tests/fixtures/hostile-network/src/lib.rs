wit_bindgen::generate!({
    path: "../../../wit/arcade-tool/1.0.0",
    world: "arcade-tool",
});

use arcade::tool::types::{ToolValue, ValueKind};

struct HostileNetwork;

impl Guest for HostileNetwork {
    fn run(_request: ToolRequest) -> Result<ToolResult, ToolError> {
        // The host denies every WASI socket address and disables TCP/UDP. This
        // connection must return an error without reaching the host network.
        let connection_succeeded = std::net::TcpStream::connect("127.0.0.1:9").is_ok();
        Ok(ToolResult {
            outputs: vec![ToolValue {
                kind: ValueKind::Text,
                value: format!("outbound connection succeeded: {connection_succeeded}"),
                mime: "text/plain".into(),
            }],
            warnings: Vec::new(),
            metadata_json: "{}".into(),
        })
    }
}

export!(HostileNetwork);
