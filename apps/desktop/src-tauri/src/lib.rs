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

use commands::{pipelines::*, shortcut::*};
use instance::*;
pub use instance::{attach_console, print_status};
use island::*;
pub use link::{print_manifest, serve_oneshot};
use tray::*;

mod about;
mod artifact_image;
mod clipboard_history;
mod commands;
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
pub mod memory;
mod open_url;
mod paste_plain;
mod portable_settings;
mod screen_capture;
mod shortcut_syntax;
mod tray;
#[cfg(target_os = "linux")]
mod wayland_shortcut;
mod window_pin;

struct ResidentInstance {
    _lock: std::fs::File,
}

// TEMPORARY_MOTION_PROBE_START
#[tauri::command]
fn profile_motion(sample: String) {
    if std::env::var_os("ARCADE_MOTION_PROBE").is_some() {
        eprintln!("MOTION_PROFILE {sample}");
    }
}
// TEMPORARY_MOTION_PROBE_END

pub fn ui_check_enabled() -> bool {
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
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
                memory::trim_while_open(webview.app_handle());
            }
        })
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
                    memory::reclaim_after(app.handle(), memory::RELEASE_AFTER_STARTUP);
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
            commands::tools::list_tools,
            commands::tools::search_tools,
            commands::plugins::choose_plugin_package,
            commands::plugins::preview_plugin,
            commands::plugins::list_plugins,
            commands::plugins::install_plugin,
            commands::plugins::uninstall_plugin,
            commands::pipelines::list_pipelines,
            commands::pipelines::save_pipeline,
            commands::pipelines::delete_pipeline,
            commands::pipelines::run_pipeline,
            commands::engines::list_providers,
            commands::tools::get_history,
            commands::tools::list_favorites,
            commands::tools::set_favorite,
            commands::tools::set_alias,
            commands::tools::get_preference,
            commands::tools::set_preference,
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
            commands::shortcut::shortcut_status,
            commands::shortcut::set_shortcut,
            commands::tools::list_jobs,
            commands::tools::start_job,
            commands::tools::cancel_job,
            commands::files::select_files,
            commands::files::choose_output_directory,
            commands::files::select_input_folder,
            commands::files::revoke_input_folder,
            commands::screen::terminate_process,
            commands::screen::window_pin_status,
            commands::engines::tesseract_download_size,
            commands::engines::download_tesseract,
            commands::screen::set_window_pin,
            commands::files::revoke_output_directory,
            commands::files::save_artifact_as,
            commands::files::save_folder,
            commands::files::choose_save_folder,
            commands::screen::screen_capture_status,
            commands::screen::run_screen_tool,
            commands::screen::screen_recording_status,
            commands::screen::start_screen_recording,
            commands::screen::stop_screen_recording,
            commands::screen::cancel_screen_recording,
            screen_capture::screen_image_preview,
            screen_capture::sample_screen_image_pixel,
            screen_capture::measure_screen_area,
            screen_capture::pin_screen_capture,
            commands::files::reveal_artifact,
            commands::files::open_artifact,
            commands::tools::run_context_action,
            commands::files::copy_text,
            open_reviewed_url,
            commands::tools::detect_context,
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
        if let tauri::RunEvent::WindowEvent {
            event: tauri::WindowEvent::Destroyed,
            ..
        } = &event
        {
            memory::reclaim_after(handle, memory::RELEASE_AFTER_CLOSE);
        }
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
