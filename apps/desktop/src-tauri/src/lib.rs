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
    sync::{Arc, Mutex},
};
#[cfg(not(target_os = "linux"))]
use tauri::menu::{Menu, MenuItem};
#[cfg(not(target_os = "linux"))]
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
pub mod installer;
mod installer_ui;
mod link;
mod media_preview;
mod open_url;
mod paste_plain;
mod portable_settings;
mod screen_capture;
#[cfg(target_os = "linux")]
mod wayland_shortcut;
mod window_pin;

// Called only on pipeline workers. GTK's callback keeps the main loop free;
// the portal dialog backend would require an unrelated Zenity executable.
fn confirm_pipeline_effects(app: &tauri::AppHandle, message: String) -> bool {
    #[cfg(target_os = "linux")]
    {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        if app
            .run_on_main_thread(move || {
                use gtk::prelude::*;
                let dialog = gtk::MessageDialog::new(
                    None::<&gtk::Window>,
                    gtk::DialogFlags::MODAL,
                    gtk::MessageType::Question,
                    gtk::ButtonsType::OkCancel,
                    &message,
                );
                dialog.set_title("Confirm pipeline effects");
                dialog.set_default_response(gtk::ResponseType::Ok);
                dialog.connect_response(move |dialog, response| {
                    let _ = send.send(response == gtk::ResponseType::Ok);
                    dialog.close();
                });
                dialog.show_all();
                dialog.present();
            })
            .is_err()
        {
            return false;
        }
        receive.recv().unwrap_or(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_dialog::MessageDialogButtons;
        app.dialog()
            .message(message)
            .title("Confirm pipeline effects")
            .buttons(MessageDialogButtons::OkCancel)
            .blocking_show()
    }
}

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
        "trigger_description":state.current.as_ref().map(shortcut_label)
    })
}

/// A shortcut as people write it (`Ctrl+Alt+B`, not `control+alt+KeyB`),
/// in a form that also parses back.
fn shortcut_label(shortcut: &Shortcut) -> String {
    let mac = cfg!(target_os = "macos");
    let mut parts: Vec<String> = [
        (Modifiers::CONTROL, "Ctrl"),
        (Modifiers::ALT, if mac { "Option" } else { "Alt" }),
        (Modifiers::SHIFT, "Shift"),
        (Modifiers::SUPER, if mac { "Cmd" } else { "Super" }),
    ]
    .into_iter()
    .filter(|(flag, _)| shortcut.mods.contains(*flag))
    .map(|(_, name)| name.to_owned())
    .collect();
    let key = shortcut.key.to_string();
    parts.push(
        key.strip_prefix("Key")
            .or_else(|| key.strip_prefix("Digit"))
            .unwrap_or(&key)
            .to_owned(),
    );
    parts.join("+")
}

#[tauri::command]
fn set_shortcut(
    trigger: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<serde_json::Value, String> {
    let trigger = arcade_link::accelerator::normalize(&trigger).map_err(|e| e.to_string())?;
    let _change = link::settings_change_lock();
    let previous = runtime
        .storage()
        .setting("global_shortcut")
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                "Super+Shift+Space".into()
            } else {
                "Ctrl+Alt+Space".into()
            }
        });
    let result = apply_shortcut_on_ui(&app, &trigger)?;
    if let Err(error) = runtime.storage().set_setting("global_shortcut", &trigger) {
        let _ = apply_shortcut_on_ui(&app, &previous);
        return Err(error.to_string());
    }
    if let Err(error) = link::publish_settings(&runtime) {
        let _ = runtime.storage().set_setting("global_shortcut", &previous);
        let _ = apply_shortcut_on_ui(&app, &previous);
        let _ = link::publish_settings(&runtime);
        return Err(error.to_string());
    }
    let status = shortcut_status(app.clone());
    let _ = app.emit("arcade://shortcut-status", status.clone());
    let _ = result;
    Ok(status)
}

#[cfg(target_os = "linux")]
fn portal_accelerator(canonical: &str) -> String {
    canonical
        .split('+')
        .map(|part| match part {
            "Ctrl" => "CTRL".into(),
            "Alt" => "ALT".into(),
            "Shift" => "SHIFT".into(),
            "Super" => "LOGO".into(),
            key => key.to_ascii_lowercase(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

fn manual_shortcut(hint: String) -> arcade_link::app::ShortcutSetResult {
    arcade_link::app::ShortcutSetResult {
        applied: false,
        via: arcade_link::app::ShortcutVia::Manual,
        hint: Some(hint),
    }
}

fn apply_shortcut_on_ui(
    app: &tauri::AppHandle,
    canonical: &str,
) -> Result<arcade_link::app::ShortcutSetResult, String> {
    #[cfg(target_os = "linux")]
    if let Some(controller) = app.try_state::<wayland_shortcut::WaylandShortcutController>() {
        if canonical.contains(' ') {
            return Ok(manual_shortcut(String::from(
                "Bind `arcade-desktop --toggle` in your desktop's keyboard settings; this portal accepts one chord.",
            )));
        }
        controller.set_preferred_trigger(portal_accelerator(canonical))?;
        return Ok(arcade_link::app::ShortcutSetResult {
            applied: true,
            via: arcade_link::app::ShortcutVia::Portal,
            hint: None,
        });
    }
    if canonical.contains(' ') {
        return Ok(manual_shortcut(
            "Bind `arcade-desktop --toggle` in your desktop's keyboard settings; native registration accepts one chord.".into(),
        ));
    }
    let shortcut = match canonical.parse::<Shortcut>() {
        Ok(shortcut) if !shortcut.mods.is_empty() => shortcut,
        _ => {
            return Ok(manual_shortcut(
                "Bind `arcade-desktop --toggle` in your desktop's keyboard settings.".into(),
            ));
        }
    };
    let Some(controller) = app.try_state::<NativeShortcutController>() else {
        return Ok(manual_shortcut(
            "Bind `arcade-desktop --toggle` in your desktop's keyboard settings.".into(),
        ));
    };
    let mut state = controller
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.current != Some(shortcut) {
        if let Err(error) = app.global_shortcut().register(shortcut) {
            state.message = format!("Shortcut needs manual registration: {error}");
            return Ok(manual_shortcut(
                "Bind `arcade-desktop --toggle` in your desktop's keyboard settings.".into(),
            ));
        }
        if let Some(previous) = state.current {
            let _ = app.global_shortcut().unregister(previous);
        }
        state.current = Some(shortcut);
    }
    state.message = "Arcade Box global shortcut is registered".into();
    Ok(arcade_link::app::ShortcutSetResult {
        applied: true,
        via: arcade_link::app::ShortcutVia::Native,
        hint: None,
    })
}

pub(crate) fn set_shortcut_from_link(
    app: &tauri::AppHandle,
    accelerator: String,
) -> Result<arcade_link::app::ShortcutSetResult, arcade_link::LinkError> {
    let canonical = arcade_link::accelerator::normalize(&accelerator)
        .map_err(|error| arcade_link::LinkError::denied(&error.to_string()))?;
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let app_handle = app.clone();
    app.run_on_main_thread(move || {
        let result = apply_shortcut_on_ui(&app_handle, &canonical)
            .map_err(arcade_link::LinkError::unavailable);
        let _ = send.send(result);
    })
    .map_err(|error| arcade_link::LinkError::unavailable(error.to_string()))?;
    receive
        .recv()
        .map_err(|error| arcade_link::LinkError::unavailable(error.to_string()))?
}

#[tauri::command]
fn list_tools(runtime: tauri::State<'_, Arc<Arcade>>) -> Vec<ToolManifest> {
    runtime
        .list_tools()
        .into_iter()
        .filter(desktop_tool_supported)
        .collect()
}

fn desktop_tool_supported(tool: &ToolManifest) -> bool {
    // macOS has no supported API to pin another application's window.
    !(cfg!(target_os = "macos") && tool.id == "arcade.system.window-pin")
}

#[tauri::command]
fn search_tools(query: String, runtime: tauri::State<'_, Arc<Arcade>>) -> Vec<ToolManifest> {
    runtime
        .search_tools(&query)
        .into_iter()
        .filter(desktop_tool_supported)
        .collect()
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

#[tauri::command(async)]
fn list_pipelines(runtime: tauri::State<'_, Arc<Arcade>>) -> Result<Vec<Pipeline>, String> {
    runtime.list_pipelines().map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn save_pipeline(
    pipeline: Pipeline,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Pipeline, String> {
    let saved = runtime
        .save_pipeline(pipeline)
        .map_err(|error| error.to_string())?;
    link::refresh(&app);
    Ok(saved)
}

#[tauri::command(async)]
fn delete_pipeline(
    id: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    runtime
        .delete_pipeline(&id)
        .map_err(|error| error.to_string())?;
    link::refresh(&app);
    Ok(())
}

#[tauri::command(async)]
fn run_pipeline(
    request_id: String,
    id: String,
    inputs: Vec<ToolValue>,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<std::collections::HashMap<String, Vec<ToolValue>>, String> {
    let active = link::OutboundRequest::begin(request_id)?;
    runtime
        .run_saved_pipeline(&id, inputs, &active.cancelled)
        .map_err(|error| error.to_string())
}

#[tauri::command(async)]
fn list_providers(
    recheck: Option<bool>,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Vec<ProviderInfo> {
    let mut providers = discover_ffmpeg(None);
    providers.extend(discover_qpdf(None));
    providers.extend(discover_vips(None));
    providers.extend(arcade_core::provider::discover_tesseract());
    providers.extend(arcade_core::provider::discover_arcade(&runtime));
    // Only when the user opens Engines: re-checking every provider runs each
    // tool's `--version`, which startup must not pay for.
    if recheck.unwrap_or(false) {
        link::providers_checked(&app);
    }
    providers
}

/// Size in MB of the Tesseract download offered on this platform, if any.
#[tauri::command]
fn tesseract_download_size() -> Option<u32> {
    arcade_link::engines::tesseract_source().map(|source| source.size_mb)
}

/// Gets Tesseract for every Arcade app (nothing is shipped with Box).
#[tauri::command(async)]
async fn download_tesseract() -> Result<String, String> {
    use arcade_link::engines::{Download, download_tesseract};
    match tauri::async_runtime::spawn_blocking(download_tesseract)
        .await
        .map_err(|error| error.to_string())??
    {
        Download::Ready(_) => Ok("Tesseract is ready.".into()),
        Download::InstallerOpened => {
            Ok("Finish the Tesseract installer, then open Engines again.".into())
        }
    }
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
async fn screen_capture_status(
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<screen_capture::ScreenCaptureStatus, String> {
    let mut status = screen_capture::capability_status().await;
    if arcade_core::link::consumer::peer_action(&runtime, arcade_link::ids::LENS, "lens.capture")
        .is_some()
    {
        status.capture_available = true;
        status.selection_mode = "Arcade Lens";
        status.message = "Select a screen region locally with Arcade Lens.".into();
    }
    Ok(status)
}

#[tauri::command(async)]
async fn run_screen_tool(
    request_id: String,
    tool_id: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Option<ToolResult>, String> {
    let runtime = runtime.inner().clone();
    let active = link::OutboundRequest::begin(request_id)?;
    let window = app.get_webview_window("island");
    if let Some(window) = &window {
        // Lens and the native picker both capture the desktop immediately, so
        // Box must be unmapped before either selection path starts.
        if window.hide().is_ok() {
            for _ in 0..50 {
                if !window.is_visible().unwrap_or(false) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
    }
    let result =
        screen_capture::run_screen_tool(&tool_id, runtime, active.cancelled.clone(), app.clone())
            .await;
    let handed_over = result
        .as_ref()
        .ok()
        .and_then(|r| r.as_ref())
        .is_some_and(|r| r.metadata.get("handedOver") == Some(&serde_json::json!(true)));
    if !handed_over {
        if let Some(window) = window {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
    result
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
        eprintln!("MOTION_PROFILE {sample}");
    }
}
// TEMPORARY_MOTION_PROBE_END

#[tauri::command]
fn hide_island(window: tauri::WebviewWindow) -> Result<(), String> {
    request_hide_island(&window, ISLAND_IDLE_RELEASE);
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
    window
        .cursor_position()
        .ok()
        .and_then(|cursor| window.monitor_from_point(cursor.x, cursor.y).ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())
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
    } else {
        // Created for a request that was cancelled before the page loaded.
        schedule_island_release(&window, ISLAND_IDLE_RELEASE);
    }
    if PENDING_SETTINGS.swap(false, std::sync::atomic::Ordering::SeqCst) {
        let _ = window.emit("arcade://open-settings", ());
    }
    if PENDING_PIPELINES.swap(false, std::sync::atomic::Ordering::SeqCst) {
        let _ = window.emit("arcade://open-pipelines", ());
    }
    let link_opens = std::mem::take(
        &mut *PENDING_LINK_OPENS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    for open in link_opens {
        let _ = window.emit("arcade://link-open", open);
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
        let hiding = {
            let mut state = ISLAND_VISIBILITY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !state.ready && state.pending_show {
                // A second press while the WebView is still loading.
                state.pending_show = false;
                return;
            }
            state.hiding
        };
        if hiding || !island.is_visible().unwrap_or(false) {
            show_island(&island);
        } else {
            request_hide_island(&island, ISLAND_IDLE_RELEASE);
        }
    });
}

/// The Island's WebView is created on demand and destroyed once it has been
/// hidden this long, so a resident Box holds no WebKit/WebView2 processes
/// while idle (SPEC §17.5). The short grace keeps quick re-toggles instant.
const ISLAND_IDLE_RELEASE: std::time::Duration = std::time::Duration::from_secs(15);

/// `box.open` requests that arrived before the Island's WebView was ready.
static PENDING_LINK_OPENS: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

/// Runs `action` on the main thread with the Island, creating its WebView
/// first when none exists. Nothing in Box's background paths (tray, Link,
/// jobs) needs the WebView, so this is only called for UI requests.
pub(crate) fn with_island(
    app: &tauri::AppHandle,
    action: impl FnOnce(&tauri::WebviewWindow) + Send + 'static,
) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = island_window(&handle) {
            action(&window);
        }
    });
}

/// Shows the Island, creating it if needed.
pub(crate) fn open_island(app: &tauri::AppHandle) {
    with_island(app, show_island);
}

/// Toggles the Island, creating it (shown) if needed.
pub(crate) fn toggle_island_app(app: &tauri::AppHandle) {
    with_island(app, toggle_island);
}

/// Delivers a `box.open` request to the Island, now or once it has loaded.
pub(crate) fn open_link_request(app: &tauri::AppHandle, open: serde_json::Value) {
    with_island(app, move |window| {
        let ready = ISLAND_VISIBILITY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ready;
        show_island(window);
        if ready {
            let _ = window.emit("arcade://link-open", open);
        } else {
            PENDING_LINK_OPENS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(open);
        }
    });
}

/// The existing Island, or a new one built from its `tauri.conf.json` entry
/// (`"create": false`). Must run on the main thread.
fn island_window(app: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window("island") {
        return Some(window);
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "island")?
        .clone();
    {
        let mut state = ISLAND_VISIBILITY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.ready = false;
        state.hiding = false;
    }
    // A pending lean restart gives way to the new WebView.
    ISLAND_RELEASES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let window = match tauri::WebviewWindowBuilder::from_config(app, &config)
        .and_then(tauri::WebviewWindowBuilder::build)
    {
        Ok(window) => window,
        Err(error) => {
            eprintln!("Could not create Arcade Island: {error}");
            return None;
        }
    };
    let close_window = window.clone();
    let destroyed_app = app.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            // Closing the window releases its WebView; Box stays resident.
            api.prevent_close();
            request_hide_island(&close_window, std::time::Duration::ZERO);
        }
        tauri::WindowEvent::Destroyed => {
            {
                let mut state = ISLAND_VISIBILITY
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.ready = false;
                state.hiding = false;
            }
            reclaim_island_memory(destroyed_app.clone());
            schedule_lean_restart(destroyed_app.clone());
        }
        _ => {}
    });
    let _ = apply_surface_mode("compact", &window, true);
    Some(window)
}

/// Destroys the Island's WebView after `delay` unless it was shown again.
fn schedule_island_release(window: &tauri::WebviewWindow, delay: std::time::Duration) {
    let generation = ISLAND_VISIBILITY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .generation;
    let island = window.clone();
    tauri::async_runtime::spawn(async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        let release = island.clone();
        let _ = island.run_on_main_thread(move || {
            let state = ISLAND_VISIBILITY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let shown_again = state.generation != generation || state.hiding || state.pending_show;
            drop(state);
            if !shown_again && !release.is_visible().unwrap_or(false) {
                let _ = release.destroy();
            }
        });
    });
}

/// How long the Island's WebView stays released before Box sheds its WebKit
/// state with a restart.
const LEAN_RESTART_AFTER: std::time::Duration = std::time::Duration::from_secs(20);

/// Finished jobs keep their results in memory only; a restart waits this long
/// after the last one so reopening the Island still shows it.
const RESULTS_KEPT_FOR: std::time::Duration = std::time::Duration::from_secs(300);

static ISLAND_RELEASES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_JOB_UPDATE: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Destroying the Island's WebView leaves WebKit's context, its network
/// process and the grown heap loaded: about 60 MiB more than a Box that never
/// opened the Island. Once nothing needs the process, restart quietly into
/// the background; the next Island costs one fresh WebView (well under a
/// second).
fn schedule_lean_restart(app: tauri::AppHandle) {
    if !cfg!(target_os = "linux") || std::env::var_os("ARCADE_BOX_NO_LEAN_RESTART").is_some() {
        return;
    }
    use std::sync::atomic::Ordering;
    let generation = ISLAND_RELEASES.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(LEAN_RESTART_AFTER);
            if ISLAND_RELEASES.load(Ordering::SeqCst) != generation
                || !app.webview_windows().is_empty()
            {
                return;
            }
            let recent_result = LAST_JOB_UPDATE
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some_and(|at| at.elapsed() < RESULTS_KEPT_FOR);
            if recent_result || link::busy(&app) {
                continue;
            }
            let restart_app = app.clone();
            let _ = app.run_on_main_thread(move || {
                // Checked again on the main thread, where the Island is built.
                if ISLAND_RELEASES.load(Ordering::SeqCst) == generation
                    && restart_app.webview_windows().is_empty()
                    && !link::busy(&restart_app)
                {
                    restart(&restart_app);
                }
            });
            return;
        }
    });
}

/// Lets GTK/WebKit finish deferred destruction, then returns freed heap pages
/// to the system once. No periodic timer.
fn reclaim_island_memory(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if app.get_webview_window("island").is_none() {
            #[cfg(all(target_os = "linux", target_env = "gnu"))]
            // SAFETY: malloc_trim is thread-safe and only releases free memory.
            unsafe {
                libc::malloc_trim(0);
            }
        }
    });
}

fn request_hide_island(window: &tauri::WebviewWindow, release_after: std::time::Duration) {
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
                    state.hiding = false;
                } else {
                    let _ = finish.emit("arcade://island-hidden", ());
                    state.hiding = false;
                    drop(state);
                    schedule_island_release(&finish, release_after);
                }
            });
        });
    });
}

#[cfg(not(target_os = "linux"))]
struct PreparedRecoveryTray {
    menu: Menu<tauri::Wry>,
    icon: tauri::image::Image<'static>,
}

#[cfg(not(target_os = "linux"))]
struct RecoveryTray {
    prepared: Option<PreparedRecoveryTray>,
    icon: Option<tauri::tray::TrayIcon>,
}

#[cfg(not(target_os = "linux"))]
thread_local! {
    static RECOVERY_TRAY: std::cell::RefCell<Option<RecoveryTray>> = const { std::cell::RefCell::new(None) };
}

/// Construct toolkit resources before the first host answer, but delay OS
/// tray registration until Box owns its icon (tray-icon has no hidden ctor).
#[cfg(not(target_os = "linux"))]
fn prepare_recovery_tray<M: tauri::Manager<tauri::Wry>>(
    app: &M,
) -> tauri::Result<PreparedRecoveryTray> {
    let show = MenuItem::with_id(app, "show-island", "Open Box", true, None::<&str>)?;
    let pipelines = MenuItem::with_id(app, "open-pipelines", "Open pipelines", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Open Settings", true, None::<&str>)?;
    let restart_item = MenuItem::with_id(app, "restart", "Restart Arcade Box", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Arcade Box", true, None::<&str>)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &pipelines,
            &settings,
            &restart_item,
            &separator,
            &quit,
        ],
    )?;
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))?;
    Ok(PreparedRecoveryTray { menu, icon })
}

fn set_recovery_tray(app: &tauri::AppHandle, state: arcade_link::trayhost::TrayState) {
    #[cfg(target_os = "linux")]
    linux_tray::set_state(app, state);
    #[cfg(not(target_os = "linux"))]
    set_native_tray(app, state);
}

/// Linux draws the tray over D-Bus with ksni. Only Own registers an item:
/// hosts such as Caelestia and Waybar still show Passive items, and Tauri's
/// libappindicator item stays registered (Passive) after removal, so a hosted
/// icon is unregistered by shutting the ksni service down.
#[cfg(target_os = "linux")]
mod linux_tray {
    use arcade_link::trayhost::TrayState;
    use ksni::TrayMethods;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::AppHandle;

    struct Tray {
        app: AppHandle,
    }

    fn on_main(app: &AppHandle, action: fn(&AppHandle)) {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || action(&handle));
    }
    fn show(app: &AppHandle) {
        super::open_island(app);
    }
    fn quit(app: &AppHandle) {
        app.exit(0);
    }

    impl ksni::Tray for Tray {
        fn id(&self) -> String {
            "arcade-box".into()
        }
        fn title(&self) -> String {
            "Arcade Box".into()
        }
        fn category(&self) -> ksni::Category {
            ksni::Category::ApplicationStatus
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            static ICONS: OnceLock<Vec<ksni::Icon>> = OnceLock::new();
            ICONS
                .get_or_init(|| {
                    image::load_from_memory(include_bytes!("../icons/icon.png"))
                        .map(|image| {
                            [32, 64, 128]
                                .into_iter()
                                .map(|size| {
                                    let image = image
                                        .resize_exact(
                                            size,
                                            size,
                                            image::imageops::FilterType::Lanczos3,
                                        )
                                        .into_rgba8();
                                    let mut data = image.into_vec();
                                    // RGBA → ARGB32 in network byte order.
                                    for pixel in data.chunks_exact_mut(4) {
                                        pixel.rotate_right(1);
                                    }
                                    ksni::Icon {
                                        width: size as i32,
                                        height: size as i32,
                                        data,
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .clone()
        }
        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "Arcade Box".into(),
                ..Default::default()
            }
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            on_main(&self.app, super::open_settings);
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::StandardItem;
            let item = |label: &str, action: fn(&AppHandle)| -> ksni::MenuItem<Self> {
                StandardItem {
                    label: label.into(),
                    activate: Box::new(move |tray: &mut Self| on_main(&tray.app, action)),
                    ..Default::default()
                }
                .into()
            };
            vec![
                item("Open Box", show),
                item("Open pipelines", super::open_pipelines),
                item("Open Settings", super::open_settings),
                item("Restart Arcade Box", super::restart),
                ksni::MenuItem::Separator,
                item("Quit Arcade Box", quit),
            ]
        }
    }

    /// The latest requested state; one serialized task reconciles to it, so an
    /// old item is gone before a new one registers.
    static VISIBLE: AtomicBool = AtomicBool::new(false);
    static HANDLE: OnceLock<tauri::async_runtime::Mutex<Option<ksni::Handle<Tray>>>> =
        OnceLock::new();

    pub fn set_state(app: &AppHandle, state: TrayState) {
        VISIBLE.store(state == TrayState::Own, Ordering::SeqCst);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut handle = HANDLE
                .get_or_init(|| tauri::async_runtime::Mutex::new(None))
                .lock()
                .await;
            if handle.as_ref().is_some_and(ksni::Handle::is_closed) {
                *handle = None;
            }
            let visible = VISIBLE.load(Ordering::SeqCst);
            if visible && handle.is_none() {
                match (Tray { app }).assume_sni_available(true).spawn().await {
                    Ok(spawned) => *handle = Some(spawned),
                    Err(error) => eprintln!("Arcade Box tray registration failed: {error}"),
                }
            } else if !visible {
                if let Some(old) = handle.take() {
                    old.shutdown().await;
                }
            }
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn set_native_tray(app: &tauri::AppHandle, state: arcade_link::trayhost::TrayState) {
    RECOVERY_TRAY.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let prepared = prepare_recovery_tray(app).map_err(|error| error.to_string());
            match prepared {
                Ok(prepared) => {
                    *slot = Some(RecoveryTray {
                        prepared: Some(prepared),
                        icon: None,
                    })
                }
                Err(error) => {
                    eprintln!("Arcade Box tray preparation failed: {error}");
                    return;
                }
            }
        }
        let Some(tray) = slot.as_mut() else { return };
        match state {
            arcade_link::trayhost::TrayState::Hosted | arcade_link::trayhost::TrayState::None => {
                if let Some(icon) = &tray.icon {
                    if let Err(error) = icon.set_visible(false) {
                        eprintln!("Arcade Box tray hide failed: {error}");
                    }
                }
            }
            arcade_link::trayhost::TrayState::Own => {
                if tray.icon.is_none() {
                    let prepared = match tray.prepared.take() {
                        Some(prepared) => prepared,
                        None => match prepare_recovery_tray(app) {
                            Ok(prepared) => prepared,
                            Err(error) => {
                                eprintln!("Arcade Box tray preparation failed: {error}");
                                return;
                            }
                        },
                    };
                    let menu_on_left = cfg!(target_os = "macos");
                    let built = TrayIconBuilder::with_id("arcade-box-tray")
                        .tooltip("Arcade Box")
                        .menu(&prepared.menu)
                        .icon(prepared.icon)
                        .show_menu_on_left_click(menu_on_left)
                        .on_menu_event(|app, event| match event.id().as_ref() {
                            "show-island" => open_island(app),
                            "open-pipelines" => open_pipelines(app),
                            "settings" => open_settings(app),
                            "restart" => restart(app),
                            "quit" => app.exit(0),
                            _ => {}
                        })
                        .on_tray_icon_event(move |tray, event| {
                            if let tauri::tray::TrayIconEvent::Click {
                                button: tauri::tray::MouseButton::Left,
                                button_state: tauri::tray::MouseButtonState::Up,
                                ..
                            } = event
                                && !menu_on_left
                            {
                                open_settings(tray.app_handle());
                            }
                        })
                        .build(app);
                    match built {
                        Ok(icon) => tray.icon = Some(icon),
                        Err(error) => {
                            eprintln!("Arcade Box tray registration failed: {error}");
                            tray.prepared = prepare_recovery_tray(app).ok();
                            return;
                        }
                    }
                }
                if let Some(icon) = &tray.icon {
                    if let Err(error) = icon.set_visible(true) {
                        eprintln!("Arcade Box tray show failed: {error}");
                    }
                }
            }
        }
    });
}

fn start_tray_host_watcher(app: &tauri::AppHandle, runtime: &Arc<Arcade>) -> Result<(), String> {
    use arcade_link::trayhost::{TrayHostWatcher, WatcherConfig};
    let app_for_updates = app.clone();
    let watcher = TrayHostWatcher::start(
        WatcherConfig {
            locations: arcade_link::Locations::discover(),
            app: arcade_link::PeerInfo {
                id: arcade_link::ids::BOX.into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            link_enabled: arcade_core::link::LinkSettings::load(runtime).enabled,
            has_tray: true,
        },
        move |state| {
            link::set_tray_state(state);
            let app = app_for_updates.clone();
            let _ = app_for_updates.run_on_main_thread(move || set_recovery_tray(&app, state));
        },
    )
    .map_err(|error| error.to_string())?;
    app.manage(watcher);
    Ok(())
}

/// Build the tray state before watching. A hosted startup retains the menu and
/// image without registering a duplicate OS icon.
#[cfg(not(target_os = "linux"))]
fn initialize_recovery_tray(app: &tauri::App) -> tauri::Result<()> {
    let prepared = prepare_recovery_tray(app)?;
    RECOVERY_TRAY.with(|slot| {
        *slot.borrow_mut() = Some(RecoveryTray {
            prepared: Some(prepared),
            icon: None,
        });
    });
    Ok(())
}

/// Set on the successor so it waits for this process to exit before taking
/// the single-instance slot and the profile lock.
const RESTART_ENV: &str = "ARCADE_BOX_RESTART_AFTER";

/// Starts a new background instance, then quits. Quitting without a
/// successor would silently remove Box, so a failed launch keeps this one.
fn restart(app: &tauri::AppHandle) {
    let started = std::process::Command::new(arcade_link::manifest::current_executable())
        .arg("--background")
        .env(RESTART_ENV, std::process::id().to_string())
        .spawn();
    match started {
        Ok(_) => app.exit(0),
        Err(error) => {
            eprintln!("Arcade Box could not start a new instance, so it kept running: {error}")
        }
    }
}

/// In a successor started by [`restart`]: waits (up to 5 s) for the old
/// instance to exit.
fn wait_for_predecessor() {
    let Some(pid) = std::env::var(RESTART_ENV)
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
    else {
        return;
    };
    for _ in 0..50 {
        if !process_alive(pid) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(target_os = "linux")]
fn process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(target_os = "macos")]
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    // SAFETY: plain Win32 calls on a handle we own and close.
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let alive = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        CloseHandle(handle);
        alive
    }
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

pub use link::{print_manifest, serve_oneshot};

/// `--settings` asked for before the Island's WebView was ready.
static PENDING_SETTINGS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PENDING_PIPELINES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Opens Settings in the Island (now, or once the WebView is ready).
pub(crate) fn open_settings(app: &tauri::AppHandle) {
    open_island_view(app, &PENDING_SETTINGS, "arcade://open-settings");
}

pub(crate) fn open_pipelines(app: &tauri::AppHandle) {
    open_island_view(app, &PENDING_PIPELINES, "arcade://open-pipelines");
}

fn open_island_view(
    app: &tauri::AppHandle,
    pending: &'static std::sync::atomic::AtomicBool,
    event: &'static str,
) {
    with_island(app, move |window| {
        let ready = ISLAND_VISIBILITY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ready;
        if !ready {
            pending.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        show_island(window);
        if ready {
            let _ = window.emit(event, ());
        }
    });
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
        Some("--restart") => restart(app),
        Some("--pipelines") => open_pipelines(app),
        Some("--toggle") => toggle_island_app(app),
        Some("--background") => {}
        _ => open_island(app),
    }
}

pub fn run(args: Vec<String>) {
    wait_for_predecessor();
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
            let db_dir = link::data_dir().ok_or("could not locate Arcade Box data directory")?;
            std::fs::create_dir_all(&db_dir)?;
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(db_dir.join("desktop.lock"))?;
            match fs2::FileExt::try_lock_exclusive(&lock) {
                Ok(()) => {
                    app.manage(ResidentInstance { _lock: lock });
                    // Started from the menu, at login or by Tools, stderr goes
                    // nowhere: keep it in the data folder instead.
                    if let Err(error) =
                        arcade_link::logs::capture_stderr(&db_dir.join("logs"), "arcade-box")
                    {
                        eprintln!("Arcade Box log unavailable: {error}");
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    eprintln!("Arcade Box is already running. Use its shortcut or tray icon.");
                    app.handle().exit(0);
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
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
            let confirmation_app = app.handle().clone();
            runtime.set_pipeline_confirmation(move |pipeline, effects| {
                let message = format!(
                    "Run \"{}\"?\n\nThis pipeline can: {}.\n\nApprove once for this version and these effects.",
                    pipeline.name,
                    effects.iter().map(|effect| match effect.as_str() {
                        "sends-to-device" => "send content to your devices",
                        "network" => "access the network",
                        "executes-commands" => "run commands",
                        "writes-files" => "create files",
                        "opens-ui" => "open app windows",
                        "clipboard" => "change the clipboard",
                        "uploads-content" => "upload content",
                        "persists" => "save data",
                        "launch-apps" => "launch apps",
                        other => other,
                    }).collect::<Vec<_>>().join(", ")
                );
                confirm_pipeline_effects(&confirmation_app, message)
            });
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
                    *LAST_JOB_UPDATE
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                        Some(std::time::Instant::now());
                    link::job_update(&event_app, &snapshot);
                    let _ = event_app.emit("arcade://job-update", snapshot);
                }),
            )?;
            app.manage(runtime);
            app.manage(jobs);
            app.manage(screen_capture::ScreenRecorder::default());
            // The Linux tray host shows the GLib application name as the
            // item's title; it defaults to the binary name.
            #[cfg(target_os = "linux")]
            gtk::glib::set_application_name("Arcade Box");
            #[cfg(not(target_os = "linux"))]
            if let Err(error) = initialize_recovery_tray(app) {
                eprintln!("Arcade Box recovery tray unavailable: {error}");
            }
            if let Err(error) = start_tray_host_watcher(
                app.handle(),
                app.state::<Arc<Arcade>>().inner(),
            ) {
                eprintln!("Arcade Box tray host watcher unavailable: {error}");
                link::set_tray_state(arcade_link::trayhost::TrayState::Own);
                set_recovery_tray(app.handle(), arcade_link::trayhost::TrayState::Own);
            }
            // The Island's WebView is not created here: `island_window` builds
            // it on the first UI request, so a background start costs no WebView.
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
            } else if first_flag.as_deref() == Some("--pipelines") {
                open_pipelines(app.handle());
            } else if first_flag.as_deref() == Some("--toggle") {
                toggle_island_app(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            link::connected_apps,
            link::set_link_settings,
            link::get_connected_app,
            link::result_link_actions,
            link::invoke_result_link_action,
            link::cancel_result_link_action,
            link::overlap_state,
            link::pipeline_link_actions,
            link::shortcut_owner,
            link::pick_peer_clipboard,
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
            tesseract_download_size,
            download_tesseract,
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
        #[cfg(not(target_os = "linux"))]
        let _ = handle;
        // Box is resident (tray, shortcut, Link): destroying its last window,
        // the Island or a pinned image, must not end the process. Explicit
        // quits pass an exit code and are not prevented.
        if let tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } = &event
        {
            api.prevent_exit();
        }
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
                    toggle_island_app(handle);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_labels_are_readable_and_parse_back() {
        for (shortcut, label) in [
            (
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyB),
                "Ctrl+Alt+B",
            ),
            (
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Digit7),
                "Ctrl+Shift+7",
            ),
            (
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space),
                "Ctrl+Alt+Space",
            ),
        ] {
            if cfg!(not(target_os = "macos")) {
                assert_eq!(shortcut_label(&shortcut), label);
            }
            assert_eq!(
                shortcut_label(&shortcut).parse::<Shortcut>().unwrap(),
                shortcut
            );
        }
    }
}
