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

use instance::*;
pub use instance::{attach_console, print_status};
use island::*;
pub use link::{print_manifest, serve_oneshot};
use tray::*;

mod about;
mod artifact_image;
mod clipboard_history;
mod dashboard;
#[cfg(target_os = "linux")]
mod hyprland;
pub mod installer;
mod installer_ui;
mod instance;
mod island;
mod link;
mod login;
mod media_preview;
mod open_url;
mod paste_plain;
mod portable_settings;
mod screen_capture;
mod shortcut_syntax;
mod tray;
#[cfg(target_os = "linux")]
mod wayland_shortcut;
mod window_pin;

// Called only on pipeline workers. GTK's callback keeps the main loop free;
// the portal dialog backend would require an unrelated Zenity executable.
// The dialog layout follows the kit's confirm dialog: the question, what the
// pipeline can do as a list, then Cancel and the primary "Run pipeline".
fn confirm_pipeline_effects(app: &tauri::AppHandle, question: String, detail: String) -> bool {
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
                    gtk::ButtonsType::None,
                    &question,
                );
                dialog.set_title("Run pipeline — Arcade Box");
                dialog.set_secondary_text(Some(&detail));
                dialog.add_button("Cancel", gtk::ResponseType::Cancel);
                if let Some(run) = dialog
                    .add_button("Run pipeline", gtk::ResponseType::Ok)
                    .downcast_ref::<gtk::Button>()
                {
                    run.style_context().add_class("suggested-action");
                }
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
            .message(format!("{question}\n\n{detail}"))
            .title("Run pipeline — Arcade Box")
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Run pipeline".into(),
                "Cancel".into(),
            ))
            .blocking_show()
    }
}

/// What a pipeline's effects mean, one per line, for the confirm dialog.
fn effects_detail(effects: &[String]) -> String {
    let lines: Vec<String> = effects
        .iter()
        .map(|effect| {
            let text = match effect.as_str() {
                "sends-to-device" => "Send content to your devices",
                "network" => "Use the network",
                "executes-commands" => "Run commands",
                "writes-files" => "Create files",
                "opens-ui" => "Open app windows",
                "clipboard" => "Change the clipboard",
                "uploads-content" => "Upload content",
                "persists" => "Save data",
                "launch-apps" => "Start apps",
                other => other,
            };
            format!("• {text}")
        })
        .collect();
    format!(
        "It can:\n{}\n\nYou're asked once for this version of the pipeline.",
        lines.join("\n")
    )
}

struct ResidentInstance {
    _lock: std::fs::File,
}

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
    let trigger = shortcut_syntax::canonical_trigger(&trigger)?;
    let _change = link::settings_change_lock();
    let previous = runtime
        .storage()
        .setting("global_shortcut")
        .ok()
        .flatten()
        .and_then(|saved| shortcut_syntax::canonical_trigger(&saved).ok())
        .unwrap_or_else(|| shortcut_syntax::DEFAULT.into());
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
        let trigger = match shortcut_syntax::xdg_trigger(canonical) {
            Ok(trigger) => trigger,
            Err(error) => {
                return Ok(manual_shortcut(format!(
                    "{error}. Bind `arcade-desktop --toggle` in your desktop's keyboard settings."
                )));
            }
        };
        controller.set_preferred_trigger(trigger)?;
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
    // BR-8: every system engine an implemented tool calls, so a missing one shows before a tool fails.
    providers.extend(arcade_core::provider::discover_poppler());
    providers.extend(arcade_core::provider::discover_ytdlp());
    providers.extend(arcade_core::provider::discover_curl());
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
    let mut dialog = app.dialog().file().set_file_name(default_name);
    if let Some(folder) = save_folder_setting(&runtime) {
        dialog = dialog.set_directory(folder);
    }
    let Some(destination) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    let destination = destination.into_path().map_err(|error| error.to_string())?;
    runtime
        .save_artifact_as(&token, &destination)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// Settings › General › Save folder: where "Save…" starts, when it still
/// exists. Unset, the dialog starts where the desktop chooses.
const SAVE_FOLDER: &str = "save_folder";

fn save_folder_setting(runtime: &Arcade) -> Option<std::path::PathBuf> {
    runtime
        .storage()
        .setting(SAVE_FOLDER)
        .ok()
        .flatten()
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_dir())
}

#[tauri::command]
fn save_folder(runtime: tauri::State<'_, Arc<Arcade>>) -> Option<String> {
    save_folder_setting(&runtime).map(|path| path.to_string_lossy().into_owned())
}

/// Picks the save folder, or clears it with `reset`.
#[tauri::command(async)]
async fn choose_save_folder(
    reset: bool,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Option<String>, String> {
    if reset {
        runtime
            .storage()
            .set_setting(SAVE_FOLDER, "")
            .map_err(|error| format!("Couldn't reset the save folder: {error}"))?;
        return Ok(None);
    }
    let mut dialog = app.dialog().file();
    if let Some(folder) = save_folder_setting(&runtime) {
        dialog = dialog.set_directory(folder);
    }
    let Some(folder) = dialog.blocking_pick_folder() else {
        return Ok(save_folder_setting(&runtime).map(|path| path.to_string_lossy().into_owned()));
    };
    let folder = folder.into_path().map_err(|error| error.to_string())?;
    runtime
        .storage()
        .set_setting(SAVE_FOLDER, &folder.to_string_lossy())
        .map_err(|error| format!("Couldn't save the folder: {error}"))?;
    Ok(Some(folder.to_string_lossy().into_owned()))
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

fn ui_check_enabled() -> bool {
    std::env::var("ARCADE_UI_CHECK").as_deref() == Ok("1")
        && std::env::var_os("ARCADE_HOME").is_some_and(|root| !root.is_empty())
}

/// The package gate exercises the real WebView, permissions and tool runtime.
/// It is opt-in and requires an explicitly isolated profile.
#[tauri::command]
fn ui_check_report(report: String, app: tauri::AppHandle) {
    if ui_check_enabled() {
        eprintln!("UI_CHECK {report}");
        app.exit(if report == "passed" { 0 } else { 1 });
    }
}

#[tauri::command]
fn open_reviewed_url(url: String, app: tauri::AppHandle) -> Result<(), String> {
    open_url::open_reviewed_url(app, url)
}

/// The OS identifier (STANDARD §10): bundle id, desktop file, window class.
pub const OS_ID: &str = "dev.arcade.Box";

/// Keeps WebView storage in Box's cache folder; Tauri's default is a folder
/// named after the OS identifier beside the user's other apps.
pub(crate) fn in_box_folder<'a, R: tauri::Runtime, M: tauri::Manager<R>>(
    builder: tauri::WebviewWindowBuilder<'a, R, M>,
) -> tauri::WebviewWindowBuilder<'a, R, M> {
    let builder = if ui_check_enabled() {
        builder.initialization_script(include_str!("ui-check.js"))
    } else {
        builder
    };
    match arcade_core::dirs::app_dirs() {
        Some(dirs) => builder.data_directory(dirs.cache.join("webview")),
        None => builder,
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn release_webview_ipc_on_destroy(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    use gtk::glib::{gobject_ffi, subclass::SignalId, translate::IntoGlib};
    use gtk::prelude::{ObjectExt, ObjectType, WidgetExt};
    use webkit2gtk::WebViewExt;

    window.with_webview(|native| {
        native.inner().connect_destroy(|webview| {
            let Some(manager) = webview.user_content_manager() else {
                return;
            };
            let Some(signal) = SignalId::lookup("script-message-received", manager.type_()) else {
                return;
            };
            // Wry's IPC signal closure owns a strong WebView reference, while
            // that view owns this manager. Break the cycle only on destruction.
            // Each Wry view has its own manager, even when contexts are shared.
            // SAFETY: manager stays strongly owned here on the GTK thread. The
            // signal id belongs to its type; only handlers of that signal on
            // this retiring manager are disconnected. Individual lookup works
            // on older GLib versions where disconnect_matched lacks ID support.
            unsafe {
                let instance = manager.as_ptr().cast();
                loop {
                    let handler = gobject_ffi::g_signal_handler_find(
                        instance,
                        gobject_ffi::G_SIGNAL_MATCH_ID,
                        signal.into_glib(),
                        0,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                    if handler == 0 {
                        break;
                    }
                    gobject_ffi::g_signal_handler_disconnect(instance, handler);
                }
            }
        });
    })?;
    Ok(())
}

/// Settings, Pipelines and the other pages live in the dashboard window.
pub(crate) fn open_settings(app: &tauri::AppHandle) {
    dashboard::open(app, Some("settings"));
}

pub(crate) fn open_pipelines(app: &tauri::AppHandle) {
    dashboard::open(app, Some("pipelines"));
}

pub(crate) fn open_dashboard(app: &tauri::AppHandle) {
    dashboard::open(app, None);
}

pub fn run(args: Vec<String>) {
    wait_for_predecessor();
    // GTK takes the Wayland app_id and the X11 WM_CLASS from the program
    // name, which is otherwise the executable's.
    #[cfg(target_os = "linux")]
    gtk::glib::set_prgname(Some(OS_ID));
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
            // Before the lock and the database: they live in the new folder.
            let (db_dir, migration) = match arcade_core::dirs::prepare() {
                Ok(prepared) => prepared,
                Err(error) => {
                    eprintln!("Arcade Box didn't start: {error}.");
                    app.handle().exit(0);
                    return Ok(());
                }
            };
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
                    for line in arcade_core::dirs::describe(&migration) {
                        eprintln!("{line}");
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
                confirm_pipeline_effects(
                    &confirmation_app,
                    format!("Run \"{}\"?", pipeline.name),
                    effects_detail(effects),
                )
            });
            let clipboard_history =
                clipboard_history::ClipboardHistory::open(db_dir.join("clipboard-history.json"))
                    .map_err(std::io::Error::other)?;
            app.manage(clipboard_history.clone());
            clipboard_history.start_monitor(app.handle().clone());
            // Normalise on load: builds before 0.4.0 could save the portal's
            // `CTRL+ALT+space` spelling, which the native path can't parse.
            let saved_shortcut = runtime
                .storage()
                .setting("global_shortcut")?
                .and_then(|saved| match shortcut_syntax::canonical_trigger(&saved) {
                    Ok(canonical) => {
                        if canonical != saved {
                            let _ = runtime.storage().set_setting("global_shortcut", &canonical);
                        }
                        Some(canonical)
                    }
                    Err(error) => {
                        eprintln!("Arcade Box ignores the saved shortcut {saved:?}: {error}");
                        None
                    }
                });
            // Existing macOS users keep 0.3's default (⇧⌘Space): it was never
            // saved, so record it once; new installs get ⌃⌥Space.
            #[cfg(target_os = "macos")]
            let saved_shortcut = saved_shortcut.or_else(|| {
                runtime
                    .storage()
                    .setting("onboarding_complete")
                    .ok()
                    .flatten()?;
                let previous = shortcut_syntax::MACOS_PREVIOUS_DEFAULT;
                let _ = runtime.storage().set_setting("global_shortcut", previous);
                Some(previous.into())
            });
            let event_app = app.handle().clone();
            let jobs = JobManager::new(
                runtime.clone(),
                Arc::new(move |snapshot| {
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
            if let Err(error) =
                start_tray_host_watcher(app.handle(), app.state::<Arc<Arcade>>().inner())
            {
                eprintln!("Arcade Box tray host watcher unavailable: {error}");
                link::set_tray_state(arcade_link::trayhost::TrayState::Own);
                set_recovery_tray(app.handle(), arcade_link::trayhost::TrayState::Own);
            }
            // The Island's WebView is not created here: `island_window` builds
            // it on the first UI request, so a background start costs no WebView.
            let default_shortcut =
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
            #[cfg(target_os = "linux")]
            if wayland_shortcut::is_wayland_session() {
                let preferred = saved_shortcut
                    .as_deref()
                    .and_then(|saved| shortcut_syntax::xdg_trigger(saved).ok())
                    .unwrap_or_else(|| {
                        shortcut_syntax::xdg_trigger(shortcut_syntax::DEFAULT)
                            .expect("the default shortcut has a portal spelling")
                    });
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
            } else if first_flag.as_deref() == Some("--dashboard") {
                open_dashboard(app.handle());
            } else if first_flag.as_deref() == Some("--toggle") {
                toggle_island_app(app.handle());
            } else if first_flag.as_deref() == Some("--show") {
                open_island(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            about::about_info,
            about::diagnostics,
            about::open_logs,
            about::open_tools,
            login::login_item,
            login::set_login_item,
            dashboard::dashboard_ready,
            dashboard::open_island,
            dashboard::open_dashboard,
            dashboard::open_tool_in_island,
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
            save_folder,
            choose_save_folder,
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
            island::hide_island,
            island::island_ready,
            ui_check_report,
            set_island_input_region,
            island::set_surface_mode
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
