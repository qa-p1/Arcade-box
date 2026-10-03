wit_bindgen::generate!({
    path: "../../../wit/arcade-tool/1.0.0",
    world: "arcade-tool",
});

struct HostileSpin;

impl Guest for HostileSpin {
    fn run(_request: ToolRequest) -> Result<ToolResult, ToolError> {
        loop {
            core::hint::spin_loop();
        }
    }
}

export!(HostileSpin);
