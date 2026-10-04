use arcade_contract::{
    ContextSuggestion, ImplementationStatus, ToolManifest, ToolRequest, ToolResult, ToolValue,
    ValueKind,
};
use arcade_core::grants::{SelectedDirectory, SelectedFile};
use arcade_core::jobs::{JobManager, JobSnapshot};
use arcade_core::provider::{ProviderInfo, discover_ffmpeg, discover_qpdf, discover_vips};
use arcade_core::storage::HistoryEntry;
use arcade_core::{Arcade, PluginInstallApproval, PluginPermissionGrant, pipeline::Pipeline};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicBool},
};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, LogicalSize, Manager, PhysicalPosition, Position, Size};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_opener::OpenerExt;

mod artifact_image;
mod clipboard_history;
#[cfg(target_os = "linux")]
mod hyprland;
mod link;
mod media_preview;
mod open_url;
mod paste_plain;
mod screen_capture;
#[cfg(target_os = "linux")]
mod wayland_shortcut;
mod window_pin;

#[derive(Clone, Copy, PartialEq)]
struct SurfaceGeometry {
    width: f64,
    height: f64,
    centered: bool,
}

#[derive(Default)]
struct IslandVisibility {
    generation: u64,
    hiding: bool,
    ready: bool,
    pending_show: bool,
}
static ISLAND_VISIBILITY: Mutex<IslandVisibility> = Mutex::new(IslandVisibility {
    generation: 0,
    hiding: false,
    ready: false,
    pending_show: false,
});
struct ResidentInstance {
    _lock: std::fs::File,
}

static SURFACE_GEOMETRY: Mutex<SurfaceGeometry> = Mutex::new(SurfaceGeometry {
    width: 740.0,
    height: 740.0,
    centered: false,
});

#[derive(Clone)]
struct NativeShortcutController {
    state: Arc<Mutex<NativeShortcutState>>,
}

struct NativeShortcutState {
    current: Option<Shortcut>,
    message: String,
}

#[tauri::command]
fn shortcut_status(app: tauri::AppHandle) -> serde_json::Value {
    #[cfg(target_os = "linux")]
    if let Some(controller) = app.try_state::<wayland_shortcut::WaylandShortcutController>() {
        return serde_json::to_value(controller.status()).unwrap_or_default();
    }
    let Some(controller) = app.try_state::<NativeShortcutController>() else {
        return serde_json::json!({"backend":"native","state":"unavailable","message":"Shortcut service is unavailable","trigger_description":null});
    };
    let state = controller
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    serde_json::json!({
        "backend":"native",
        "state": if state.current.is_some() { "registered" } else { "unavailable" },
        "message":state.message,
        "trigger_description":state.current.map(|shortcut| shortcut.to_string())
    })
}

#[tauri::command]
fn set_shortcut(
    trigger: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<serde_json::Value, String> {
    #[cfg(target_os = "linux")]
    if let Some(controller) = app.try_state::<wayland_shortcut::WaylandShortcutController>() {
        controller.set_preferred_trigger(trigger.clone())?;
        runtime
            .storage()
            .set_setting("global_shortcut", &trigger)
            .map_err(|error| error.to_string())?;
        link::refresh(&app);
        return Ok(shortcut_status(app));
    }
    let shortcut = trigger
        .parse::<Shortcut>()
        .map_err(|error| format!("Invalid shortcut: {error}"))?;
    if shortcut.mods.is_empty() {
        return Err("A global shortcut needs at least one modifier key".into());
    }
    let controller = app
        .try_state::<NativeShortcutController>()
        .ok_or("Shortcut service is unavailable")?;
    let mut state = controller
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.current != Some(shortcut) {
        app.global_shortcut()
            .register(shortcut)
            .map_err(|error| format!("Shortcut is unavailable: {error}"))?;
        if let Some(previous) = state.current {
            let _ = app.global_shortcut().unregister(previous);
        }
        state.current = Some(shortcut);
    }
    state.message = "Arcade Box global shortcut is registered".into();
    drop(state);
    runtime
        .storage()
        .set_setting("global_shortcut", &trigger)
        .map_err(|error| error.to_string())?;
    link::refresh(&app);
    let status = shortcut_status(app.clone());
    let _ = app.emit("arcade://shortcut-status", status.clone());
    Ok(status)
}

#[tauri::command]
fn list_tools(runtime: tauri::State<'_, Arc<Arcade>>) -> Vec<ToolManifest> {
    runtime.list_tools()
}

#[tauri::command]
fn search_tools(query: String, runtime: tauri::State<'_, Arc<Arcade>>) -> Vec<ToolManifest> {
    runtime.search_tools(&query)
}

#[tauri::command]
fn choose_plugin_package(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let path = folder.into_path().map_err(|error| error.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
fn preview_plugin(
    source_dir: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<arcade_core::PluginPreview, String> {
    let path = PathBuf::from(source_dir);
    if !path.is_absolute() {
        return Err("Choose an unpacked plugin package from disk".into());
    }
    runtime
        .preview_plugin(&path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_plugins(
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Vec<arcade_core::PluginSummary>, String> {
    runtime.list_plugins().map_err(|error| error.to_string())
}

#[tauri::command]
fn install_plugin(
    source_dir: String,
    grants: Vec<String>,
    acknowledge_escalation: bool,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<arcade_core::PluginSummary, String> {
    let path = PathBuf::from(source_dir);
    if !path.is_absolute() {
        return Err("Choose an unpacked plugin package from disk".into());
    }
    let grants = grants
        .into_iter()
        .map(|grant| match grant.as_str() {
            "read-user-selected" => Ok(PluginPermissionGrant::ReadUserSelectedFiles),
            _ => Err(format!("Unsupported plugin permission: {grant}")),
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    runtime
        .install_plugin(
            &path,
            PluginInstallApproval {
                grants,
                acknowledge_escalation,
            },
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn uninstall_plugin(
    plugin_id: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    runtime
        .uninstall_plugin(&plugin_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_pipelines(runtime: tauri::State<'_, Arc<Arcade>>) -> Result<Vec<Pipeline>, String> {
    runtime.list_pipelines().map_err(|error| error.to_string())
}

#[tauri::command]
fn save_pipeline(
    pipeline: Pipeline,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Pipeline, String> {
    runtime
        .save_pipeline(pipeline)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_pipeline(id: String, runtime: tauri::State<'_, Arc<Arcade>>) -> Result<(), String> {
    runtime
        .delete_pipeline(&id)
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn run_pipeline(
    id: String,
    inputs: Vec<ToolValue>,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<std::collections::HashMap<String, Vec<ToolValue>>, String> {
    runtime
        .run_saved_pipeline(&id, inputs, &AtomicBool::new(false))
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn list_providers() -> Vec<ProviderInfo> {
    let mut providers = discover_ffmpeg(None);
    providers.extend(discover_qpdf(None));
    providers.extend(discover_vips(None));
    providers
}

#[tauri::command]
fn get_history(runtime: tauri::State<'_, Arc<Arcade>>) -> Result<Vec<HistoryEntry>, String> {
    runtime
        .storage()
        .recent_history(50)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_favorites(runtime: tauri::State<'_, Arc<Arcade>>) -> Result<Vec<String>, String> {
    runtime
        .storage()
        .favorites()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_favorite(
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
fn set_alias(
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
fn get_preference(
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
fn set_preference(
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

#[tauri::command(async)]
fn run_tool(
    request: ToolRequest,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<ToolResult, String> {
    validate_frontend_request(&request, &runtime)?;
    runtime.run_tool(request).map_err(|error| error.to_string())
}

fn validate_frontend_request(request: &ToolRequest, runtime: &Arcade) -> Result<(), String> {
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
fn list_jobs(jobs: tauri::State<'_, JobManager>) -> Vec<JobSnapshot> {
    jobs.list()
}

#[tauri::command]
fn start_job(
    request: ToolRequest,
    runtime: tauri::State<'_, Arc<Arcade>>,
    jobs: tauri::State<'_, JobManager>,
) -> Result<JobSnapshot, String> {
    validate_frontend_request(&request, &runtime)?;
    jobs.submit(request).map_err(|error| error.to_string())
}

#[tauri::command]
fn cancel_job(job_id: String, jobs: tauri::State<'_, JobManager>) -> Result<JobSnapshot, String> {
    jobs.cancel(&job_id).map_err(|error| error.to_string())
}

#[tauri::command]
fn select_files(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Vec<SelectedFile>, String> {
    let Some(paths) = app.dialog().file().blocking_pick_files() else {
        return Ok(vec![]);
    };
    paths
        .into_iter()
        .map(|file| {
            let path = file.into_path().map_err(|error| error.to_string())?;
            runtime
                .grants()
                .grant(&path)
                .map_err(|error| error.to_string())
        })
        .collect()
}

#[tauri::command]
fn choose_output_directory(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Option<SelectedDirectory>, String> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let path = folder.into_path().map_err(|error| error.to_string())?;
    runtime
        .grant_output_directory(&path)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn select_input_folder(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Option<SelectedDirectory>, String> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let path = folder.into_path().map_err(|error| error.to_string())?;
    runtime
        .grant_input_directory(&path)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn revoke_input_folder(token: String, runtime: tauri::State<'_, Arc<Arcade>>) {
    runtime.revoke_input_directory(&token);
}

#[tauri::command]
fn terminate_process(
    pid: u32,
    process_name: String,
    executable: Option<String>,
    start_time: u64,
) -> Result<(), String> {
    arcade_core::system_tools::terminate_process(
        pid,
        &process_name,
        executable.as_deref(),
        start_time,
    )
}

#[tauri::command]
fn window_pin_status() -> window_pin::WindowPinStatus {
    window_pin::status()
}

#[tauri::command]
fn set_window_pin(
    pinned: bool,
    app: tauri::AppHandle,
) -> Result<window_pin::WindowPinStatus, String> {
    window_pin::set_foreground_pin(pinned, &app)
}

#[tauri::command]
fn revoke_output_directory(token: String, runtime: tauri::State<'_, Arc<Arcade>>) {
    runtime.revoke_output_directory(&token);
}

#[tauri::command(async)]
async fn save_artifact_as(
    token: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Option<SelectedFile>, String> {
    let source_path = runtime
        .grants()
        .resolve(&token)
        .map_err(|error| error.to_string())?;
    let default_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Arcade-result");
    let Some(destination) = app
        .dialog()
        .file()
        .set_file_name(default_name)
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let destination = destination.into_path().map_err(|error| error.to_string())?;
    runtime
        .save_artifact_as(&token, &destination)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
async fn screen_capture_status() -> screen_capture::ScreenCaptureStatus {
    screen_capture::capability_status().await
}

#[tauri::command(async)]
async fn run_screen_tool(
    tool_id: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Option<ToolResult>, String> {
    let runtime = runtime.inner().clone();
    screen_capture::run_screen_tool(&tool_id, runtime).await
}

#[tauri::command]
fn screen_recording_status(
    recorder: tauri::State<'_, screen_capture::ScreenRecorder>,
) -> screen_capture::ScreenRecordingSnapshot {
    recorder.snapshot()
}

#[tauri::command(async)]
async fn start_screen_recording(
    recorder: tauri::State<'_, screen_capture::ScreenRecorder>,
    runtime: tauri::State<'_, Arc<Arcade>>,
    jobs: tauri::State<'_, JobManager>,
) -> Result<Option<screen_capture::ScreenRecordingSnapshot>, String> {
    screen_capture::start_screen_recording(&recorder, runtime.inner().clone(), &jobs).await
}

#[tauri::command(async)]
async fn stop_screen_recording(
    recorder: tauri::State<'_, screen_capture::ScreenRecorder>,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<ToolResult, String> {
    screen_capture::stop_screen_recording(&recorder, runtime.inner().clone()).await
}

#[tauri::command(async)]
async fn cancel_screen_recording(
    recorder: tauri::State<'_, screen_capture::ScreenRecorder>,
) -> Result<screen_capture::ScreenRecordingSnapshot, String> {
    screen_capture::cancel_screen_recording(&recorder).await
}

#[tauri::command]
fn reveal_artifact(
    token: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    let path = runtime
        .grants()
        .resolve(&token)
        .map_err(|error| error.to_string())?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_artifact(
    token: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    let path = runtime
        .grants()
        .resolve(&token)
        .map_err(|error| error.to_string())?;
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn run_context_action(
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
fn copy_text(text: String, app: tauri::AppHandle) -> Result<(), String> {
    if text.len() > 16 * 1024 * 1024 {
        return Err("The result is too large to copy. Save it as a file instead.".into());
    }
    app.clipboard()
        .write_text(text)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn detect_context(app: tauri::AppHandle) -> Vec<ContextSuggestion> {
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

// TEMPORARY_MOTION_PROBE_START
#[tauri::command]
fn profile_motion(sample: String) {
    if std::env::var_os("ARCADE_MOTION_PROBE").is_some() {
        eprintln!("MOTION_PROFILE {}", sample);
    }
}
// TEMPORARY_MOTION_PROBE_END

#[tauri::command]
fn hide_island(window: tauri::WebviewWindow) -> Result<(), String> {
    request_hide_island(&window);
    Ok(())
}

// Async so window reconfiguration and Hyprland queries never block the WebView
// thread while the Island is animating between states.
#[tauri::command]
async fn set_surface_mode(mode: String, window: tauri::WebviewWindow) -> Result<(), String> {
    apply_surface_mode(&mode, &window, false)
}

fn apply_surface_mode(
    mode: &str,
    window: &tauri::WebviewWindow,
    force: bool,
) -> Result<(), String> {
    let (width, height, dashboard) = match mode {
        "compact" | "search" | "tool" => (740.0, 740.0, false),
        "dashboard" => (1100.0, 760.0, true),
        _ => return Err("Unknown surface mode".into()),
    };
    let (width, height) = fit_surface_size(window, width, height);
    let next = SurfaceGeometry {
        width,
        height,
        centered: dashboard,
    };
    {
        let mut geometry = SURFACE_GEOMETRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Compact, search, and tool states share one canvas. Only the web
        // surface changes between them, so skip native work entirely.
        if !force && *geometry == next {
            return Ok(());
        }
        *geometry = next;
    }
    window
        .set_min_size(Some(Size::Logical(LogicalSize::new(
            width.min(320.0),
            height.min(116.0),
        ))))
        .map_err(|error| error.to_string())?;
    window
        .set_size(Size::Logical(LogicalSize::new(width, height)))
        .map_err(|error| error.to_string())?;
    let _ = window.set_always_on_top(!dashboard);
    let _ = window.set_skip_taskbar(true);
    place_surface(window, dashboard);
    #[cfg(target_os = "linux")]
    if window.is_visible().unwrap_or(false) {
        let _ = hyprland::place_island(width, height, dashboard);
    }
    Ok(())
}

#[tauri::command]
fn open_reviewed_url(url: String, app: tauri::AppHandle) -> Result<(), String> {
    open_url::open_reviewed_url(app, url)
}

fn active_monitor(window: &tauri::WebviewWindow) -> Option<tauri::Monitor> {
    let monitor = window
        .cursor_position()
        .ok()
        .and_then(|cursor| window.monitor_from_point(cursor.x, cursor.y).ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten());
    monitor
}

fn fit_surface_size(window: &tauri::WebviewWindow, width: f64, height: f64) -> (f64, f64) {
    let Some(monitor) = active_monitor(window) else {
        return (width, height);
    };
    let scale = monitor.scale_factor().max(1.0);
    let screen = monitor.size();
    let max_width = ((screen.width as f64 / scale) - 48.0).max(1.0);
    let max_height = ((screen.height as f64 / scale) - 48.0).max(1.0);
    (width.min(max_width), height.min(max_height))
}

fn place_surface(window: &tauri::WebviewWindow, centered: bool) {
    let Some(monitor) = active_monitor(window) else {
        return;
    };
    let screen = monitor.size();
    let origin = monitor.position();
    let Ok(size) = window.outer_size() else {
        return;
    };
    let x_offset = (i64::from(screen.width) - i64::from(size.width)).max(0) / 2;
    let y_offset = if centered {
        (i64::from(screen.height) - i64::from(size.height)).max(0) / 2
    } else {
        0
    };
    let x = (i64::from(origin.x) + x_offset).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let y = (i64::from(origin.y) + y_offset).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
}

#[tauri::command]
fn island_ready(window: tauri::WebviewWindow) {
    let pending = {
        let mut state = ISLAND_VISIBILITY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.ready = true;
        std::mem::take(&mut state.pending_show)
    };
    if pending {
        show_island(&window);
    }
    if PENDING_SETTINGS.swap(false, std::sync::atomic::Ordering::SeqCst) {
        let _ = window.emit("arcade://open-settings", ());
    }
}

/// Restrict pointer input to the visible surface. The transparent canvas exists
/// for the spring/resize animation; it must never block the desktop beneath it.
#[tauri::command]
fn set_island_input_region(
    window: tauri::WebviewWindow,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if [x, y, width, height]
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 32768.0)
        || width < 0.0
        || height < 0.0
    {
        return Err("Invalid island input bounds".into());
    }
    #[cfg(target_os = "linux")]
    {
        let island = window.clone();
        window
            .run_on_main_thread(move || {
                use gtk::prelude::WidgetExt;
                if let Ok(native) = island.gtk_window() {
                    let region =
                        gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(
                            x.floor() as i32,
                            y.floor() as i32,
                            width.ceil() as i32,
                            height.ceil() as i32,
                        ));
                    native.input_shape_combine_region(Some(&region));
                }
            })
            .map_err(|error| error.to_string())?;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = window;
    Ok(())
}

pub(crate) fn show_island(window: &tauri::WebviewWindow) {
    let island = window.clone();
    let _ = window.run_on_main_thread(move || {
        {
            let mut state = ISLAND_VISIBILITY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !state.ready {
                state.pending_show = true;
                return;
            }
            state.generation = state.generation.wrapping_add(1);
            state.hiding = false;
        }
        let geometry = *SURFACE_GEOMETRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (width, height) = fit_surface_size(&island, geometry.width, geometry.height);
        let _ = island.set_size(Size::Logical(LogicalSize::new(width, height)));
        place_surface(&island, geometry.centered);
        if let Err(error) = island.show() {
            eprintln!("Could not show Arcade Island: {error}");
            return;
        }
        let _ = island.set_focus();
        // TEMPORARY_MOTION_EVAL_START
        if std::env::var_os("ARCADE_MOTION_PROBE").is_some() {
            let _ = island.eval(include_str!("motion-probe.js"));
        }
        // TEMPORARY_MOTION_EVAL_END
        let _ = island.emit("arcade://island-shown", ());
    });
}

pub(crate) fn toggle_island(window: &tauri::WebviewWindow) {
    let island = window.clone();
    let _ = window.run_on_main_thread(move || {
        let hiding = ISLAND_VISIBILITY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .hiding;
        if hiding || !island.is_visible().unwrap_or(false) {
            show_island(&island);
        } else {
            request_hide_island(&island);
        }
    });
}

fn request_hide_island(window: &tauri::WebviewWindow) {
    let island = window.clone();
    let _ = window.run_on_main_thread(move || {
        let generation = {
            let mut state = ISLAND_VISIBILITY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.hiding {
                return;
            }
            state.hiding = true;
            state.generation
        };
        let _ = island.emit("arcade://island-hiding", ());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(140)).await;
            let finish = island.clone();
            let _ = island.run_on_main_thread(move || {
                let mut state = ISLAND_VISIBILITY
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                // A second shortcut can reopen the island during its exit.
                if state.generation != generation || !state.hiding {
                    return;
                }
                if let Err(error) = finish.hide() {
                    eprintln!("Could not hide Arcade Island: {error}");
                    let _ = finish.emit("arcade://island-shown", ());
                } else {
                    let _ = finish.emit("arcade://island-hidden", ());
                }
                state.hiding = false;
            });
        });
    });
}

fn install_recovery_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show-island", "Show Arcade Island", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Arcade Box", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut builder = TrayIconBuilder::with_id("arcade-box-tray").menu(&menu);
    // The bundled image is 16-bit RGBA. Tauri's default window image keeps
    // those raw bytes, but tray-icon expects 8-bit RGBA; decode here so the
    // tray receives the correctly normalized pixel buffer.
    let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))?;
    builder = builder.icon(tray_icon);
    builder
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show-island" => {
                if let Some(window) = app.get_webview_window("island") {
                    show_island(&window);
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

/// On Windows the release build has no console; attach to the parent's so
/// `--version` and `--arcade-manifest` print where they were asked.
pub fn attach_console() {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
}

pub use link::print_manifest;

/// `--settings` asked for before the Island's WebView was ready.
static PENDING_SETTINGS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Opens Settings in the Island (now, or once the WebView is ready).
fn open_settings(app: &tauri::AppHandle) {
    let ready = ISLAND_VISIBILITY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .ready;
    if !ready {
        PENDING_SETTINGS.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    if let Some(window) = app.get_webview_window("island") {
        show_island(&window);
    }
    if ready {
        let _ = app.emit("arcade://open-settings", ());
    }
}

/// Another launch of Arcade Box (the applications menu, `--settings`,
/// `--quit`) talks to this instance instead of starting a second one.
fn handle_second_instance(app: &tauri::AppHandle, argv: &[String]) {
    let flag = argv
        .iter()
        .skip(1)
        .find(|a| a.starts_with("--"))
        .map(String::as_str);
    match flag {
        Some("--quit") => app.exit(0),
        Some("--settings") => open_settings(app),
        Some("--background") => {}
        _ => {
            if let Some(window) = app.get_webview_window("island") {
                show_island(&window);
            }
        }
    }
}

pub fn run(args: Vec<String>) {
    let first_flag = args.iter().find(|a| a.starts_with("--")).cloned();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            handle_second_instance(app, &argv);
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // `--quit` with nothing running: exit without starting anything.
            if first_flag.as_deref() == Some("--quit") {
                app.handle().exit(0);
                return Ok(());
            }
            let db_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&db_dir)?;
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(db_dir.join("desktop.lock"))?;
            match lock.try_lock() {
                Ok(()) => {
                    app.manage(ResidentInstance { _lock: lock });
                }
                Err(std::fs::TryLockError::WouldBlock) => {
                    eprintln!("Arcade Box is already running. Use its shortcut or tray icon.");
                    app.handle().exit(0);
                    return Ok(());
                }
                Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
            }
            #[cfg(target_os = "linux")]
            {
                if let Err(error) = hyprland::install_surface_rules() {
                    eprintln!("Arcade Island compositor setup failed: {error}");
                }
            }
            #[cfg(target_os = "linux")]
            let wayland_identity_result = wayland_shortcut::is_wayland_session()
                .then(|| wayland_shortcut::register_host_app(app.handle()));

            let runtime = Arc::new(Arcade::open(&db_dir.join("arcade.sqlite3"))?);
            let clipboard_history =
                clipboard_history::ClipboardHistory::open(db_dir.join("clipboard-history.json"))
                    .map_err(std::io::Error::other)?;
            app.manage(clipboard_history.clone());
            clipboard_history.start_monitor(app.handle().clone());
            let saved_shortcut = runtime.storage().setting("global_shortcut")?;
            let event_app = app.handle().clone();
            let jobs = JobManager::new(
                runtime.clone(),
                Arc::new(move |snapshot| {
                    let _ = event_app.emit("arcade://job-update", snapshot);
                }),
            )?;
            app.manage(runtime);
            app.manage(jobs);
            app.manage(screen_capture::ScreenRecorder::default());
            if let Err(error) = install_recovery_tray(app) {
                eprintln!("Arcade Box recovery tray unavailable: {error}");
            }
            {
                if let Some(window) = app.get_webview_window("island") {
                    let close_window = window.clone();
                    window.on_window_event(move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            request_hide_island(&close_window);
                        }
                    });
                }
            }
            if let Some(window) = app.get_webview_window("island") {
                let _ = apply_surface_mode("compact", &window, true);
            }
            let default_shortcut = if cfg!(target_os = "macos") {
                Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Space)
            } else {
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space)
            };
            #[cfg(target_os = "linux")]
            if wayland_shortcut::is_wayland_session() {
                let preferred = saved_shortcut.unwrap_or_else(|| "CTRL+ALT+space".into());
                let controller = wayland_shortcut::start(
                    app.handle().clone(),
                    preferred,
                    wayland_identity_result.unwrap_or(Ok(())),
                );
                app.manage(controller);
            } else {
                let preferred = saved_shortcut
                    .and_then(|shortcut| shortcut.parse::<Shortcut>().ok())
                    .unwrap_or(default_shortcut);
                app.manage(register_native_shortcut(app.handle(), preferred)?);
            }
            #[cfg(not(target_os = "linux"))]
            {
                let preferred = saved_shortcut
                    .and_then(|shortcut| shortcut.parse::<Shortcut>().ok())
                    .unwrap_or(default_shortcut);
                app.manage(register_native_shortcut(app.handle(), preferred)?);
            }
            link::start(app.handle());
            if first_flag.as_deref() == Some("--settings") {
                open_settings(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_tools,
            search_tools,
            choose_plugin_package,
            preview_plugin,
            list_plugins,
            install_plugin,
            uninstall_plugin,
            list_pipelines,
            save_pipeline,
            delete_pipeline,
            run_pipeline,
            list_providers,
            get_history,
            list_favorites,
            set_favorite,
            set_alias,
            get_preference,
            set_preference,
            clipboard_history::clipboard_history_status,
            clipboard_history::list_clipboard_history,
            clipboard_history::set_clipboard_history_enabled,
            clipboard_history::set_clipboard_history_retention,
            clipboard_history::set_clipboard_history_exclusions,
            clipboard_history::pin_clipboard_history_item,
            clipboard_history::delete_clipboard_history_item,
            clipboard_history::clear_clipboard_history,
            clipboard_history::copy_clipboard_history_item,
            artifact_image::image_result_preview,
            media_preview::video_preview,
            media_preview::estimate_video_output,
            media_preview::audio_preview,
            media_preview::measure_audio_loudness,
            artifact_image::copy_image_result,
            paste_plain::paste_plain_status,
            paste_plain::paste_plain_text,
            shortcut_status,
            set_shortcut,
            run_tool,
            list_jobs,
            start_job,
            cancel_job,
            select_files,
            choose_output_directory,
            select_input_folder,
            revoke_input_folder,
            terminate_process,
            window_pin_status,
            set_window_pin,
            revoke_output_directory,
            save_artifact_as,
            screen_capture_status,
            run_screen_tool,
            screen_recording_status,
            start_screen_recording,
            stop_screen_recording,
            cancel_screen_recording,
            screen_capture::screen_image_preview,
            screen_capture::sample_screen_image_pixel,
            screen_capture::measure_screen_area,
            screen_capture::pin_screen_capture,
            reveal_artifact,
            open_artifact,
            run_context_action,
            copy_text,
            open_reviewed_url,
            detect_context,
            profile_motion,
            hide_island,
            island_ready,
            set_island_input_region,
            set_surface_mode
        ])
        .build(tauri::generate_context!())
        .expect("Arcade Box desktop runtime failed to initialize");
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            link::stop();
        }
        #[cfg(target_os = "linux")]
        if let tauri::RunEvent::Exit = event {
            if handle.try_state::<ResidentInstance>().is_some() {
                hyprland::remove_global_binding();
                hyprland::remove_surface_rules();
            }
        }
    });
}

fn register_native_shortcut(
    app: &tauri::AppHandle,
    shortcut: Shortcut,
) -> tauri::Result<NativeShortcutController> {
    let controller = NativeShortcutController {
        state: Arc::new(Mutex::new(NativeShortcutState {
            current: None,
            message: "Checking the global shortcut".into(),
        })),
    };
    let current = controller.state.clone();
    app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |handle, pressed, event| {
                let active = current
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .current;
                if Some(*pressed) == active && event.state() == ShortcutState::Pressed {
                    if let Some(window) = handle.get_webview_window("island") {
                        toggle_island(&window);
                    }
                }
            })
            .build(),
    )?;
    let mut state = controller
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match app.global_shortcut().register(shortcut) {
        Ok(()) => {
            state.current = Some(shortcut);
            state.message = "Arcade Box global shortcut is registered".into();
        }
        Err(error) => state.message = format!("Shortcut unavailable: {error}"),
    }
    drop(state);
    Ok(controller)
}
