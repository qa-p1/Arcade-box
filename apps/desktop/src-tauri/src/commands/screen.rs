//! Screen tools: capture, recording, window pins and process termination.

use crate::*;

#[tauri::command]
pub(crate) fn terminate_process(
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
pub(crate) fn window_pin_status() -> window_pin::WindowPinStatus {
    window_pin::status()
}

#[tauri::command]
pub(crate) fn set_window_pin(
    pinned: bool,
    app: tauri::AppHandle,
) -> Result<window_pin::WindowPinStatus, String> {
    window_pin::set_foreground_pin(pinned, &app)
}

#[tauri::command(async)]
pub(crate) async fn screen_capture_status(
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
pub(crate) async fn run_screen_tool(
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
pub(crate) fn screen_recording_status(
    recorder: tauri::State<'_, screen_capture::ScreenRecorder>,
) -> screen_capture::ScreenRecordingSnapshot {
    recorder.snapshot()
}

#[tauri::command(async)]
pub(crate) async fn start_screen_recording(
    recorder: tauri::State<'_, screen_capture::ScreenRecorder>,
    runtime: tauri::State<'_, Arc<Arcade>>,
    jobs: tauri::State<'_, JobManager>,
) -> Result<Option<screen_capture::ScreenRecordingSnapshot>, String> {
    screen_capture::start_screen_recording(&recorder, runtime.inner().clone(), &jobs).await
}

#[tauri::command(async)]
pub(crate) async fn stop_screen_recording(
    recorder: tauri::State<'_, screen_capture::ScreenRecorder>,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<ToolResult, String> {
    screen_capture::stop_screen_recording(&recorder, runtime.inner().clone()).await
}

#[tauri::command(async)]
pub(crate) async fn cancel_screen_recording(
    recorder: tauri::State<'_, screen_capture::ScreenRecorder>,
) -> Result<screen_capture::ScreenRecordingSnapshot, String> {
    screen_capture::cancel_screen_recording(&recorder).await
}
