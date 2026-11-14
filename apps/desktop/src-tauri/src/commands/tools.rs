//! The tool catalog, search, jobs, history, favourites, aliases, preferences and context actions the frontend runs.

use crate::*;

#[tauri::command]
pub(crate) fn list_tools(runtime: tauri::State<'_, Arc<Arcade>>) -> Vec<ToolManifest> {
    runtime
        .list_tools()
        .into_iter()
        .filter(desktop_tool_supported)
        .collect()
}

pub(crate) fn desktop_tool_supported(tool: &ToolManifest) -> bool {
    // macOS has no supported API to pin another application's window.
    !(cfg!(target_os = "macos") && tool.id == "arcade.system.window-pin")
}

#[tauri::command]
pub(crate) fn search_tools(
    query: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Vec<ToolManifest> {
    runtime
        .search_tools(&query)
        .into_iter()
        .filter(desktop_tool_supported)
        .collect()
}

#[tauri::command]
pub(crate) fn get_history(
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Vec<HistoryEntry>, String> {
    runtime
        .storage()
        .recent_history(50)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn list_favorites(
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Vec<String>, String> {
    runtime
        .storage()
        .favorites()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn set_favorite(
    tool_id: String,
    favorite: bool,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    if !runtime.list_tools().iter().any(|tool| tool.id == tool_id) {
        return Err("Unknown tool ID".into());
    }
    runtime
        .storage()
        .set_favorite(&tool_id, favorite)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn set_alias(
    alias: String,
    tool_id: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    if !runtime.list_tools().iter().any(|tool| tool.id == tool_id) {
        return Err("Unknown tool ID".into());
    }
    if alias.trim().is_empty() || alias.len() > 80 {
        return Err("Alias must contain 1 to 80 characters".into());
    }
    runtime
        .storage()
        .set_alias(&alias, &tool_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn get_preference(
    key: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Option<String>, String> {
    if !matches!(key.as_str(), "onboarding_complete" | "theme") {
        return Err("Unknown preference".into());
    }
    runtime
        .storage()
        .setting(&key)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn set_preference(
    key: String,
    value: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    let valid = match key.as_str() {
        "onboarding_complete" => matches!(value.as_str(), "true" | "false"),
        "theme" => matches!(value.as_str(), "system" | "light" | "dark"),
        _ => false,
    };
    if !valid {
        return Err("Unsupported preference value".into());
    }
    runtime
        .storage()
        .set_setting(&key, &value)
        .map_err(|error| error.to_string())
}

pub(crate) fn validate_frontend_request(
    request: &ToolRequest,
    runtime: &Arcade,
) -> Result<(), String> {
    if request
        .inputs
        .iter()
        .any(|input| input.kind == ValueKind::File)
    {
        return Err("Raw file paths are not accepted; use Arcade Box's scoped file picker".into());
    }
    if request
        .inputs
        .iter()
        .map(|input| input.value.len())
        .sum::<usize>()
        > 16 * 1024 * 1024
    {
        return Err("Text input exceeds the 16 MB interactive limit; use a file workflow".into());
    }
    let tool = runtime
        .list_tools()
        .into_iter()
        .find(|tool| tool.id == request.tool_id)
        .ok_or("Unknown tool ID")?;
    if tool.status == ImplementationStatus::Planned {
        return Err("This tool is not implemented yet".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn list_jobs(jobs: tauri::State<'_, JobManager>) -> Vec<JobSnapshot> {
    jobs.list()
}

#[tauri::command]
pub(crate) fn start_job(
    request: ToolRequest,
    runtime: tauri::State<'_, Arc<Arcade>>,
    jobs: tauri::State<'_, JobManager>,
) -> Result<JobSnapshot, String> {
    validate_frontend_request(&request, &runtime)?;
    jobs.submit(request).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn cancel_job(
    job_id: String,
    jobs: tauri::State<'_, JobManager>,
) -> Result<JobSnapshot, String> {
    jobs.cancel(&job_id).map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub(crate) fn run_context_action(
    tool_id: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<ToolResult, String> {
    if !matches!(
        tool_id.as_str(),
        "arcade.text.structured"
            | "arcade.text.case"
            | "arcade.text.clean"
            | "arcade.barcode.qr-generate"
    ) {
        return Err("This clipboard action is not available".into());
    }
    let text = app
        .clipboard()
        .read_text()
        .map_err(|error| error.to_string())?;
    if text.len() > 16 * 1024 * 1024 {
        return Err("Clipboard text exceeds the interactive limit".into());
    }
    let options = if tool_id == "arcade.text.structured" {
        serde_json::json!({"to": "same"})
    } else {
        serde_json::Value::Null
    };
    runtime
        .run_tool(ToolRequest {
            tool_id,
            inputs: vec![ToolValue::text(text, "text/plain")],
            options,
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn detect_context(app: tauri::AppHandle) -> Vec<ContextSuggestion> {
    let Ok(text) = app.clipboard().read_text() else {
        return vec![];
    };
    if text.len() > 16 * 1024 * 1024 {
        return vec![];
    }
    let mut suggestions = Vec::new();
    if serde_json::from_str::<serde_json::Value>(&text).is_ok() {
        suggestions.push(ContextSuggestion {
            tool_id: "arcade.text.structured".into(),
            reason: "Clipboard contains JSON".into(),
        });
    }
    if text.starts_with("https://") || text.starts_with("http://") {
        suggestions.push(ContextSuggestion {
            tool_id: "arcade.barcode.qr-generate".into(),
            reason: "Clipboard contains a URL".into(),
        });
    }
    if !text.is_empty() {
        suggestions.push(ContextSuggestion {
            tool_id: "arcade.text.case".into(),
            reason: "Clipboard contains text".into(),
        });
    }
    suggestions
}
