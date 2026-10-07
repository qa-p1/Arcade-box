//! User-initiated screen content capture through each platform's native picker.
//! Linux prefers the XDG Screenshot portal and falls back to ImageMagick on X11.

use arcade_contract::{ResultStatus, ToolRequest, ToolResult, ToolValue, ValueKind};
use arcade_core::{Arcade, grants::SelectedFile};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    future::Future,
    io::Read,
    path::PathBuf,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

const MAX_CAPTURE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CAPTURE_PIXELS: u64 = MAX_CAPTURE_BYTES / 4;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
static PIN_WINDOW_COUNTER: AtomicU64 = AtomicU64::new(1);

type CaptureFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Option<SelectedFile>, String>> + Send + 'a>>;

/// Platform capture implementations share this small boundary. The frontend
/// receives only a normal opaque file grant regardless of the native API.
pub trait ScreenCaptureProvider: Send + Sync {
    fn capture_content<'a>(&'a self, runtime: &'a Arcade) -> CaptureFuture<'a>;
}

struct PlatformCaptureProvider;

impl ScreenCaptureProvider for PlatformCaptureProvider {
    fn capture_content<'a>(&'a self, runtime: &'a Arcade) -> CaptureFuture<'a> {
        #[cfg(target_os = "linux")]
        {
            Box::pin(capture_linux_area(runtime))
        }
        #[cfg(target_os = "windows")]
        {
            Box::pin(capture_windows_area(runtime))
        }
        #[cfg(target_os = "macos")]
        {
            Box::pin(capture_macos_area(runtime))
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            Box::pin(capture_unsupported_area(runtime))
        }
    }
}

pub async fn capture_screen_area(runtime: &Arcade) -> Result<Option<SelectedFile>, String> {
    PlatformCaptureProvider.capture_content(runtime).await
}

#[tauri::command]
pub fn screen_image_preview(
    token: String,
    max_edge: u32,
    runtime: tauri::State<'_, std::sync::Arc<Arcade>>,
) -> Result<arcade_core::screen_tools::ImagePreview, String> {
    arcade_core::screen_tools::create_image_preview(runtime.grants(), &token, max_edge)
}

#[tauri::command]
pub fn sample_screen_image_pixel(
    token: String,
    x: u32,
    y: u32,
    runtime: tauri::State<'_, std::sync::Arc<Arcade>>,
) -> Result<arcade_core::screen_tools::SampledPixel, String> {
    arcade_core::screen_tools::sample_image_pixel(runtime.grants(), &token, x, y)
}

#[tauri::command]
pub fn measure_screen_area(
    token: String,
    start: [u32; 2],
    end: [u32; 2],
    runtime: tauri::State<'_, std::sync::Arc<Arcade>>,
) -> Result<ToolResult, String> {
    let measurement =
        arcade_core::screen_tools::measure_screen_points(runtime.grants(), &token, start, end)?;
    let value = serde_json::to_string(&measurement)
        .map_err(|error| format!("Could not encode screen measurement: {error}"))?;
    Ok(ToolResult {
        tool_id: "arcade.screen.ruler".into(),
        status: ResultStatus::Success,
        outputs: vec![ToolValue::text(value, "structured/screen-measurement")],
        message: Some("Measured screen pixel distances locally.".into()),
        warnings: vec![],
        metadata: BTreeMap::new(),
    })
}

#[tauri::command]
pub fn pin_screen_capture(
    token: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, std::sync::Arc<Arcade>>,
) -> Result<serde_json::Value, String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    let preview = arcade_core::screen_tools::create_image_preview(runtime.grants(), &token, 512)?;
    let preview_js = serde_json::to_string(&serde_json::json!({
        "dataUrl": preview.data_url,
        "width": preview.width,
        "height": preview.height
    }))
    .map_err(|error| format!("Could not prepare pinned image preview: {error}"))?;
    let counter = PIN_WINDOW_COUNTER.fetch_add(1, Ordering::Relaxed);
    let label = format!("arcade-pin-{}-{counter}", std::process::id());
    let width = f64::from(preview.width.min(560).max(180)) + 28.0;
    let height = f64::from(preview.height.min(340).max(120)) + 46.0;
    let script = format!("window.__ARCADE_PIN_IMAGE = {preview_js};");
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(PathBuf::from("pin.html")))
        .title("Arcade Box · Pinned reference")
        .inner_size(width, height)
        .min_inner_size(200.0, 140.0)
        .resizable(true)
        .decorations(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .initialization_script(&script)
        .build()
        .map_err(|error| format!("Could not open the pinned image window: {error}"))?;

    let message = if cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        "Pinned reference opened. The Wayland compositor may limit always-on-top behavior."
    } else {
        "Pinned reference opened above other windows."
    };
    Ok(serde_json::json!({
        "label": label,
        "message": message,
        "width": preview.width,
        "height": preview.height
    }))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenCaptureStatus {
    pub platform: &'static str,
    pub capture_available: bool,
    pub selection_mode: &'static str,
    pub message: String,
    pub recording_available: bool,
    pub recording_message: &'static str,
}

pub async fn capability_status() -> ScreenCaptureStatus {
    #[cfg(target_os = "linux")]
    {
        use ashpd::desktop::screenshot::{AvailableTargets, ScreenshotProxy};
        let portal_status: (bool, &'static str, String) = match ScreenshotProxy::new().await {
            Ok(portal) => match portal.available_targets().await {
                Ok(targets) if targets.contains(AvailableTargets::Area) => (
                    true,
                    "area",
                    "The XDG Screenshot portal supports area selection.".into(),
                ),
                Ok(_) => (
                    false,
                    "unavailable",
                    "This screenshot portal does not advertise area selection.".into(),
                ),
                Err(error) => (
                    false,
                    "unavailable",
                    format!("Could not query screenshot portal targets: {error}"),
                ),
            },
            Err(error) => (
                false,
                "unavailable",
                format!("The XDG Screenshot portal is unavailable: {error}"),
            ),
        };
        let status = if portal_status.0 {
            portal_status
        } else if let Some(executable) = linux_x11_import_executable() {
            (
                true,
                "x11-import",
                format!(
                    "The XDG Screenshot portal is unavailable; using the ImageMagick area selector at {}.",
                    executable.display()
                ),
            )
        } else {
            portal_status
        };
        let (recording_available, recording_message) = linux_recording_capability().await;
        return ScreenCaptureStatus {
            platform: "linux",
            capture_available: status.0,
            selection_mode: status.1,
            message: status.2,
            recording_available,
            recording_message,
        };
    }

    #[cfg(target_os = "windows")]
    {
        let (capture_available, message) =
            match windows_capture::capture::GraphicsCaptureApi::is_supported() {
                Ok(true) => (
                    true,
                    "Windows Graphics Capture and the system picker are available.".to_owned(),
                ),
                Ok(false) => (
                    false,
                    "This Windows version does not support Graphics Capture.".to_owned(),
                ),
                Err(error) => (
                    false,
                    format!("Could not query Windows Graphics Capture support: {error}"),
                ),
            };
        let (recording_available, recording_message) = windows_recording_capability().await;
        return ScreenCaptureStatus {
            platform: "windows",
            capture_available,
            selection_mode: if capture_available {
                "screen-or-window"
            } else {
                "unavailable"
            },
            message,
            recording_available,
            recording_message,
        };
    }

    #[cfg(target_os = "macos")]
    {
        let capture_available =
            screencapturekit::content_sharing_picker::SCContentSharingPicker::is_available();
        let recording_available =
            screencapturekit::recording_output::SCRecordingOutput::is_available();
        return ScreenCaptureStatus {
            platform: "macos",
            capture_available,
            selection_mode: if capture_available {
                "screen-window-or-app"
            } else {
                "unavailable"
            },
            message: if capture_available {
                "ScreenCaptureKit and the system content picker are available. Screen Recording permission may still be required.".into()
            } else {
                "Screen content selection requires macOS 14 or later.".into()
            },
            recording_available,
            recording_message: if recording_available {
                "ScreenCaptureKit recording output is available on macOS 15 or later."
            } else {
                "Screen recording output requires macOS 15 or later; screen selection and screenshots work on macOS 14."
            },
        };
    }

    #[allow(unreachable_code)]
    ScreenCaptureStatus {
        platform: "unsupported",
        capture_available: false,
        selection_mode: "unavailable",
        message: "Screen capture is not supported on this platform build.".into(),
        recording_available: false,
        recording_message: "Screen recording is not supported on this platform build.",
    }
}

/// The recorder owns its process, portal session and private staging directory.
/// The UI can close while recording; only the explicit stop command finalizes it.
#[derive(Clone, Default)]
pub struct ScreenRecorder {
    state: std::sync::Arc<std::sync::Mutex<RecorderState>>,
}

#[derive(Default)]
struct RecorderState {
    starting: bool,
    finalizing: bool,
    session: Option<RecordingSession>,
}

enum RecordingSession {
    #[cfg(target_os = "linux")]
    Linux(LinuxRecordingSession),
    #[cfg(target_os = "macos")]
    Mac(MacRecordingSession),
    #[cfg(target_os = "windows")]
    Windows(WindowsRecordingSession),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenRecordingSnapshot {
    pub platform: &'static str,
    pub available: bool,
    pub starting: bool,
    pub recording: bool,
    pub finalizing: bool,
    pub elapsed_seconds: Option<u64>,
    pub job_id: Option<String>,
    pub message: String,
}

impl ScreenRecorder {
    pub fn snapshot(&self) -> ScreenRecordingSnapshot {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let recording = state.session.is_some();
        let elapsed_seconds = state
            .session
            .as_ref()
            .map(RecordingSession::elapsed_seconds);
        let job_id = state.session.as_ref().map(RecordingSession::job_id);
        ScreenRecordingSnapshot {
            platform: current_platform_name(),
            available: recorder_build_available(),
            starting: state.starting,
            recording,
            finalizing: state.finalizing,
            elapsed_seconds,
            job_id,
            message: if state.starting {
                "Waiting for screen selection…".into()
            } else if state.finalizing {
                "Finalizing the recording and saving the video…".into()
            } else if recording {
                "Screen recording is active. Stop it to finalize the video.".into()
            } else if recorder_build_available() {
                "Ready to record a user-selected screen or window.".into()
            } else {
                recording_unavailable_message().into()
            },
        }
    }
}

impl RecordingSession {
    fn elapsed_seconds(&self) -> u64 {
        match self {
            #[cfg(target_os = "linux")]
            Self::Linux(session) => session.started_at.elapsed().as_secs(),
            #[cfg(target_os = "macos")]
            Self::Mac(session) => session.started_at.elapsed().as_secs(),
            #[cfg(target_os = "windows")]
            Self::Windows(session) => session.started_at.elapsed().as_secs(),
        }
    }

    fn job_id(&self) -> String {
        match self {
            #[cfg(target_os = "linux")]
            Self::Linux(session) => session.job.id().to_owned(),
            #[cfg(target_os = "macos")]
            Self::Mac(session) => session.job.id().to_owned(),
            #[cfg(target_os = "windows")]
            Self::Windows(session) => session.job.id().to_owned(),
        }
    }

    fn job(&self) -> &arcade_core::jobs::ExternalJobHandle {
        match self {
            #[cfg(target_os = "linux")]
            Self::Linux(session) => &session.job,
            #[cfg(target_os = "macos")]
            Self::Mac(session) => &session.job,
            #[cfg(target_os = "windows")]
            Self::Windows(session) => &session.job,
        }
    }
}

const fn current_platform_name() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unsupported"
    }
}

const fn recorder_build_available() -> bool {
    cfg!(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "windows"
    ))
}

const fn recording_unavailable_message() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows recording needs a compatible FFmpeg build with libx264 and MP4 support."
    } else if cfg!(target_os = "macos") {
        "Screen recording output requires macOS 15 or later. macOS 14 supports screen selection and screenshots."
    } else {
        "Screen recording is not supported on this platform build."
    }
}

/// Starts a portal-selected recording. A cancelled portal interaction returns `None`.
pub async fn start_screen_recording(
    recorder: &ScreenRecorder,
    runtime: std::sync::Arc<Arcade>,
    jobs: &arcade_core::jobs::JobManager,
) -> Result<Option<ScreenRecordingSnapshot>, String> {
    {
        let mut state = recorder
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.starting || state.finalizing || state.session.is_some() {
            return Err("A screen recording is already starting or active".into());
        }
        if !recorder_build_available() {
            return Err(recording_unavailable_message().into());
        }
        state.starting = true;
    }

    let (_job_snapshot, job) =
        match jobs.begin_external("arcade.screen.recorder", "Waiting for screen selection…") {
            Ok(job) => job,
            Err(error) => {
                recorder
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .starting = false;
                return Err(error.to_string());
            }
        };

    #[cfg(target_os = "linux")]
    let result = start_linux_recording(runtime, job.clone())
        .await
        .map(|session| session.map(RecordingSession::Linux));
    #[cfg(target_os = "macos")]
    let result = start_macos_recording(runtime, job.clone())
        .await
        .map(|session| session.map(RecordingSession::Mac));
    #[cfg(target_os = "windows")]
    let result = start_windows_recording(runtime, job.clone())
        .await
        .map(|session| session.map(RecordingSession::Windows));
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let result: Result<Option<RecordingSession>, String> =
        Err(recording_unavailable_message().into());

    let mut state = recorder
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.starting = false;
    match result {
        Ok(Some(session)) => {
            state.session = Some(session);
            drop(state);
            watch_job_cancellation(recorder.clone());
            Ok(Some(recorder.snapshot()))
        }
        Ok(None) => {
            job.request_cancellation();
            let _ = job.finish(Err("Screen selection was cancelled".into()));
            Ok(None)
        }
        Err(error) => {
            let _ = job.finish(Err(error.clone()));
            Err(error)
        }
    }
}

/// Stops the active capture, finalizes its container and publishes a normal opaque artifact.
pub async fn stop_screen_recording(
    recorder: &ScreenRecorder,
    runtime: std::sync::Arc<Arcade>,
) -> Result<ToolResult, String> {
    let session = {
        let mut state = recorder
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.starting {
            return Err("Wait for screen selection to finish before stopping".into());
        }
        if state.finalizing {
            return Err("The screen recording is already being finalized".into());
        }
        let Some(session) = state.session.take() else {
            return Err("There is no active screen recording".into());
        };
        state.finalizing = true;
        session
    };

    let job = session.job().clone();
    let result = match session {
        #[cfg(target_os = "linux")]
        RecordingSession::Linux(session) => stop_linux_recording(session, runtime).await,
        #[cfg(target_os = "macos")]
        RecordingSession::Mac(session) => stop_macos_recording(session, runtime).await,
        #[cfg(target_os = "windows")]
        RecordingSession::Windows(session) => stop_windows_recording(session, runtime).await,
    };
    let _ = job.finish(result.clone());
    recorder
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .finalizing = false;
    result
}

pub async fn cancel_screen_recording(
    recorder: &ScreenRecorder,
) -> Result<ScreenRecordingSnapshot, String> {
    let session = {
        let mut state = recorder
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.starting {
            return Err("Wait for screen selection to finish before cancelling".into());
        }
        if state.finalizing {
            return Err("The screen recording is already being finalized".into());
        }
        let Some(session) = state.session.take() else {
            return Err("There is no active screen recording".into());
        };
        state.finalizing = true;
        session
    };
    let job = session.job().clone();
    let cancel_result = match session {
        #[cfg(target_os = "linux")]
        RecordingSession::Linux(session) => cancel_linux_recording(session).await,
        #[cfg(target_os = "macos")]
        RecordingSession::Mac(session) => cancel_macos_recording(session).await,
        #[cfg(target_os = "windows")]
        RecordingSession::Windows(session) => cancel_windows_recording(session).await,
    };
    if let Err(error) = &cancel_result {
        let _ = job.finish(Err(error.clone()));
    } else {
        job.request_cancellation();
        let _ = job.finish(Err(
            "Screen recording cancelled; partial video removed".into()
        ));
    }
    let mut state = recorder
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.finalizing = false;
    drop(state);
    cancel_result?;
    Ok(recorder.snapshot())
}

fn watch_job_cancellation(recorder: ScreenRecorder) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let job = {
                let state = recorder
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state.session.as_ref().map(|session| match session {
                    #[cfg(target_os = "linux")]
                    RecordingSession::Linux(session) => session.job.clone(),
                    #[cfg(target_os = "macos")]
                    RecordingSession::Mac(session) => session.job.clone(),
                    #[cfg(target_os = "windows")]
                    RecordingSession::Windows(session) => session.job.clone(),
                })
            };
            let Some(job) = job else {
                break;
            };
            if job.is_cancellation_requested() {
                let _ = cancel_screen_recording(&recorder).await;
                break;
            }
        }
    });
}

#[cfg(target_os = "linux")]
struct LinuxRecordingSession {
    portal_session: Option<ashpd::desktop::Session<ashpd::desktop::screencast::Screencast>>,
    child: std::process::Child,
    _staging: tempfile::TempDir,
    output_path: PathBuf,
    provider_path: PathBuf,
    provider_version: String,
    started_at: std::time::Instant,
    job: arcade_core::jobs::ExternalJobHandle,
}

#[cfg(target_os = "linux")]
impl Drop for LinuxRecordingSession {
    fn drop(&mut self) {
        use std::os::unix::process::ExitStatusExt;
        if self.child.try_wait().ok().flatten().is_some() {
            return;
        }
        if let Ok(pid) = i32::try_from(self.child.id()) {
            unsafe {
                libc::kill(pid, libc::SIGINT);
            }
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) if status.code().is_some() || status.signal().is_some() => return,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                _ => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(target_os = "linux")]
struct GStreamerProvider {
    launch: PathBuf,
    version: String,
}

#[cfg(target_os = "linux")]
async fn linux_recording_capability() -> (bool, &'static str) {
    use ashpd::desktop::screencast::{Screencast, SourceType};

    let provider = tauri::async_runtime::spawn_blocking(probe_gstreamer_provider).await;
    let Ok(Ok(_provider)) = provider else {
        return (
            false,
            "Screen recording needs compatible GStreamer pipewiresrc, vp8enc and webmmux elements.",
        );
    };
    let portal = match Screencast::new().await {
        Ok(portal) => portal,
        Err(_) => {
            return (
                false,
                "The XDG ScreenCast portal is unavailable; check desktop portal and PipeWire support.",
            );
        }
    };
    match portal.available_source_types().await {
        Ok(sources)
            if sources.contains(SourceType::Monitor) || sources.contains(SourceType::Window) =>
        {
            (
                true,
                "XDG ScreenCast portal, PipeWire and GStreamer recording are available.",
            )
        }
        Ok(_) => (
            false,
            "The XDG ScreenCast portal does not advertise monitor or window capture.",
        ),
        Err(_) => (
            false,
            "The XDG ScreenCast portal could not report capture capabilities.",
        ),
    }
}

#[cfg(target_os = "windows")]
async fn windows_recording_capability() -> (bool, &'static str) {
    if !matches!(
        windows_capture::capture::GraphicsCaptureApi::is_supported(),
        Ok(true)
    ) {
        return (
            false,
            "Windows Graphics Capture is not supported by this system.",
        );
    }
    let provider = tauri::async_runtime::spawn_blocking(|| {
        arcade_core::provider::discover_ffmpeg(None)
            .into_iter()
            .find(|provider| {
                provider.compatible
                    && provider
                        .capabilities
                        .iter()
                        .any(|item| item == "encoder:libx264")
                    && provider.capabilities.iter().any(|item| item == "mux:mp4")
            })
    })
    .await;
    if matches!(provider, Ok(Some(_))) {
        (
            true,
            "Windows Graphics Capture and a compatible system FFmpeg with H.264/MP4 support are available.",
        )
    } else {
        (
            false,
            "Screen recording needs a compatible system FFmpeg with libx264 and MP4 support.",
        )
    }
}

#[cfg(target_os = "linux")]
async fn cancel_linux_recording(mut session: LinuxRecordingSession) -> Result<(), String> {
    let portal_session = session
        .portal_session
        .take()
        .ok_or("The ScreenCast portal session is already closed")?;
    let process_result = tauri::async_runtime::spawn_blocking(move || {
        let kill_result = session.child.kill();
        let wait_result = session.child.wait();
        drop(session);
        kill_result
            .and(wait_result)
            .map_err(|error| format!("Could not stop the recording process cleanly: {error}"))
    })
    .await
    .map_err(|error| format!("Could not cancel the screen recording: {error}"))?;
    let close_result = portal_session
        .close()
        .await
        .map_err(|error| format!("Could not close the ScreenCast portal session: {error}"));
    process_result?;
    close_result
}

#[cfg(target_os = "macos")]
struct MacRecordingSession {
    stream: screencapturekit::stream::SCStream,
    output: screencapturekit::recording_output::SCRecordingOutput,
    _staging: tempfile::TempDir,
    output_path: PathBuf,
    started_at: std::time::Instant,
    job: arcade_core::jobs::ExternalJobHandle,
}

#[cfg(target_os = "macos")]
async fn start_macos_recording(
    runtime: std::sync::Arc<Arcade>,
    job: arcade_core::jobs::ExternalJobHandle,
) -> Result<Option<MacRecordingSession>, String> {
    use screencapturekit::{
        async_api::AsyncSCContentSharingPicker,
        content_sharing_picker::{
            SCContentSharingPicker, SCContentSharingPickerConfiguration, SCPickerOutcome,
        },
        recording_output::{
            SCRecordingOutput, SCRecordingOutputCodec, SCRecordingOutputConfiguration,
            SCRecordingOutputFileType,
        },
        stream::{SCStream, configuration::SCStreamConfiguration},
    };

    if !SCRecordingOutput::is_available() {
        return Err(recording_unavailable_message().into());
    }
    if !SCContentSharingPicker::is_available() {
        return Err("The macOS system content picker is unavailable. This feature requires macOS 14 or later.".into());
    }
    let selected = match AsyncSCContentSharingPicker::show(
        &SCContentSharingPickerConfiguration::new(),
    )
    .await
    {
        SCPickerOutcome::Picked(selected) => selected,
        SCPickerOutcome::Cancelled => return Ok(None),
        SCPickerOutcome::Error(error) => {
            return Err(format!("Could not open the macOS screen picker: {error}"));
        }
    };
    let (width, height) = selected.pixel_size();
    checked_dimensions(width, height)?;
    let filter = selected.filter();
    let staging = staging_directory(&runtime)?;
    let output_path = staging.path().join("screen-recording.mp4");
    let config = SCStreamConfiguration::new()
        .with_width(width)
        .with_height(height);
    let recording_config = SCRecordingOutputConfiguration::new()
        .with_output_url(&output_path)
        .with_video_codec(SCRecordingOutputCodec::H264)
        .with_output_file_type(SCRecordingOutputFileType::MP4);

    let (stream, output) = tauri::async_runtime::spawn_blocking(move || {
        let output = SCRecordingOutput::new(&recording_config).ok_or_else(|| {
            "ScreenCaptureKit could not create an H.264/MP4 recording output. Check macOS version and Screen Recording permission.".to_owned()
        })?;
        let stream = SCStream::new(&filter, &config);
        stream
            .add_recording_output(&output)
            .map_err(|error| format!("Could not attach the recording output: {error}"))?;
        stream
            .start_capture()
            .map_err(|error| format!("ScreenCaptureKit could not start capture: {error}"))?;
        Ok::<_, String>((stream, output))
    })
    .await
    .map_err(|error| format!("ScreenCaptureKit initialization stopped unexpectedly: {error}"))??;
    Ok(Some(MacRecordingSession {
        stream,
        output,
        _staging: staging,
        output_path,
        started_at: std::time::Instant::now(),
        job,
    }))
}

#[cfg(target_os = "macos")]
async fn stop_macos_recording(
    session: MacRecordingSession,
    runtime: std::sync::Arc<Arcade>,
) -> Result<ToolResult, String> {
    tauri::async_runtime::spawn_blocking(move || finalize_macos_recording(session, runtime))
        .await
        .map_err(|error| format!("ScreenCaptureKit finalization stopped unexpectedly: {error}"))?
}

#[cfg(target_os = "macos")]
fn finalize_macos_recording(
    session: MacRecordingSession,
    runtime: std::sync::Arc<Arcade>,
) -> Result<ToolResult, String> {
    let MacRecordingSession {
        stream,
        output,
        _staging: staging,
        output_path,
        ..
    } = session;
    stream
        .remove_recording_output(&output)
        .map_err(|error| format!("Could not finalize the ScreenCaptureKit movie: {error}"))?;
    let metadata = fs::symlink_metadata(&output_path)
        .map_err(|error| format!("ScreenCaptureKit did not create the recording: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() < 1024 {
        return Err("ScreenCaptureKit produced an empty or invalid recording".into());
    }
    let selected = runtime
        .publish_staged_output(
            None,
            &output_path,
            "screen-recording.mp4",
            &AtomicBool::new(false),
        )
        .map_err(|error| format!("Could not keep the screen recording safely: {error}"))?;
    let _ = staging;
    Ok(recording_result(
        selected,
        "ScreenCaptureKit",
        "native-screen-capture-kit",
    ))
}

#[cfg(target_os = "macos")]
async fn cancel_macos_recording(session: MacRecordingSession) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let MacRecordingSession {
            stream,
            output,
            _staging: staging,
            ..
        } = session;
        let result = stream
            .remove_recording_output(&output)
            .map_err(|error| format!("Could not stop ScreenCaptureKit: {error}"));
        drop(staging);
        result
    })
    .await
    .map_err(|error| format!("Could not cancel ScreenCaptureKit recording: {error}"))?
}

#[cfg(target_os = "windows")]
struct WindowsRecordingSession {
    control: Option<
        windows_capture::capture::CaptureControl<
            WindowsRecordingHandler,
            Box<dyn std::error::Error + Send + Sync>,
        >,
    >,
    child: std::process::Child,
    _staging: tempfile::TempDir,
    output_path: PathBuf,
    provider_path: PathBuf,
    provider_version: String,
    started_at: std::time::Instant,
    job: arcade_core::jobs::ExternalJobHandle,
}

#[cfg(target_os = "windows")]
impl Drop for WindowsRecordingSession {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(target_os = "windows")]
struct WindowsRecorderFlags {
    writer: std::process::ChildStdin,
    width: u32,
    height: u32,
    stop: std::sync::Arc<AtomicBool>,
}

#[cfg(target_os = "windows")]
struct WindowsRecordingHandler {
    writer: Option<std::process::ChildStdin>,
    width: u32,
    height: u32,
    stop: std::sync::Arc<AtomicBool>,
    scratch: Vec<u8>,
}

#[cfg(target_os = "windows")]
impl windows_capture::capture::GraphicsCaptureApiHandler for WindowsRecordingHandler {
    type Flags = WindowsRecorderFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: windows_capture::capture::Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            writer: Some(ctx.flags.writer),
            width: ctx.flags.width,
            height: ctx.flags.height,
            stop: ctx.flags.stop,
            scratch: Vec::new(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut windows_capture::frame::Frame<'_>,
        capture_control: windows_capture::graphics_capture_api::InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if self.stop.load(std::sync::atomic::Ordering::Acquire) {
            self.writer.take();
            capture_control.stop();
            return Ok(());
        }
        if frame.width() != self.width || frame.height() != self.height {
            return Err("Windows changed the selected capture dimensions mid-recording".into());
        }
        let mut buffer = frame.buffer()?;
        let pixels = buffer.as_nopadding_buffer(&mut self.scratch);
        let writer = self
            .writer
            .as_mut()
            .ok_or_else(|| "The FFmpeg recording stream is already closed".to_owned())?;
        use std::io::Write;
        writer.write_all(pixels)?;
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        self.writer.take();
        Ok(())
    }
}

#[cfg(target_os = "windows")]
async fn start_windows_recording(
    runtime: std::sync::Arc<Arcade>,
    job: arcade_core::jobs::ExternalJobHandle,
) -> Result<Option<WindowsRecordingSession>, String> {
    tauri::async_runtime::spawn_blocking(move || start_windows_recording_blocking(runtime, job))
        .await
        .map_err(|error| format!("Windows recording startup stopped unexpectedly: {error}"))?
}

#[cfg(target_os = "windows")]
fn start_windows_recording_blocking(
    runtime: std::sync::Arc<Arcade>,
    job: arcade_core::jobs::ExternalJobHandle,
) -> Result<Option<WindowsRecordingSession>, String> {
    use windows_capture::{
        capture::GraphicsCaptureApiHandler,
        graphics_capture_picker::GraphicsCapturePicker,
        settings::{
            ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
            MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
        },
    };

    let provider = arcade_core::provider::discover_ffmpeg(None)
        .into_iter()
        .find(|provider| {
            provider.compatible
                && provider
                    .capabilities
                    .iter()
                    .any(|item| item == "encoder:libx264")
                && provider.capabilities.iter().any(|item| item == "mux:mp4")
        })
        .ok_or_else(|| recording_unavailable_message().to_owned())?;
    let Some(selected) = GraphicsCapturePicker::pick_item()
        .map_err(|error| format!("Could not open the Windows capture picker: {error}"))?
    else {
        return Ok(None);
    };
    let (width, height) = selected
        .size()
        .map_err(|error| format!("Could not inspect the selected capture target: {error}"))?;
    if width <= 0 || height <= 0 {
        return Err("The selected screen or window reports invalid dimensions".into());
    }
    checked_dimensions(width as u32, height as u32)?;
    let width = width as u32;
    let height = height as u32;
    let staging = tempfile::Builder::new()
        .prefix("arcade-screen-recording-")
        .tempdir_in(runtime.artifact_staging_root())
        .map_err(|error| format!("Could not create a private recording workspace: {error}"))?;
    let output_path = staging.path().join("screen-recording.mp4");

    let mut child = std::process::Command::new(&provider.executable_path)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pixel_format",
            "bgra",
            "-video_size",
        ])
        .arg(format!("{width}x{height}"))
        .args([
            "-framerate",
            "30",
            "-i",
            "pipe:0",
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "23",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
            "-n",
        ])
        .arg(&output_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| {
            format!(
                "Could not start FFmpeg at {}: {error}",
                provider.executable_path.display()
            )
        })?;
    let writer = child
        .stdin
        .take()
        .ok_or("Could not open FFmpeg's frame input")?;
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let flags = WindowsRecorderFlags {
        writer,
        width,
        height,
        stop,
    };
    let settings = Settings::new(
        selected.item,
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Custom(std::time::Duration::from_millis(33)),
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        flags,
    );
    let control = match WindowsRecordingHandler::start_free_threaded(settings) {
        Ok(control) => control,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Windows Graphics Capture could not start: {error}"));
        }
    };
    if let Some(status) = child
        .try_wait()
        .map_err(|error| format!("Could not verify FFmpeg startup: {error}"))?
    {
        let _ = control.stop();
        return Err(format!("FFmpeg stopped during startup (status {status})"));
    }
    Ok(Some(WindowsRecordingSession {
        control: Some(control),
        child,
        _staging: staging,
        output_path,
        provider_path: provider.executable_path,
        provider_version: provider.version,
        started_at: std::time::Instant::now(),
        job,
    }))
}

#[cfg(target_os = "windows")]
async fn stop_windows_recording(
    session: WindowsRecordingSession,
    runtime: std::sync::Arc<Arcade>,
) -> Result<ToolResult, String> {
    tauri::async_runtime::spawn_blocking(move || finalize_windows_recording(session, runtime))
        .await
        .map_err(|error| format!("Windows recording finalization stopped unexpectedly: {error}"))?
}

#[cfg(target_os = "windows")]
fn finalize_windows_recording(
    mut session: WindowsRecordingSession,
    runtime: std::sync::Arc<Arcade>,
) -> Result<ToolResult, String> {
    session
        .control
        .take()
        .ok_or("The Windows capture session is already stopped")?
        .stop()
        .map_err(|error| format!("Could not stop the Windows capture stream: {error}"))?;
    wait_for_process(&mut session.child, std::time::Duration::from_secs(20))?;
    let metadata = fs::symlink_metadata(&session.output_path)
        .map_err(|error| format!("FFmpeg did not create the recording: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() < 1024 {
        return Err("FFmpeg produced an empty or invalid screen recording".into());
    }
    let selected = runtime
        .publish_staged_output(
            None,
            &session.output_path,
            "screen-recording.mp4",
            &AtomicBool::new(false),
        )
        .map_err(|error| format!("Could not keep the screen recording safely: {error}"))?;
    let mut result = recording_result(
        selected,
        &session.provider_path.display().to_string(),
        "windows-graphics-capture",
    );
    result
        .metadata
        .insert("providerVersion".into(), json!(session.provider_version));
    Ok(result)
}

#[cfg(target_os = "windows")]
async fn cancel_windows_recording(mut session: WindowsRecordingSession) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(control) = session.control.take() {
            control
                .stop()
                .map_err(|error| format!("Could not stop Windows capture cleanly: {error}"))?;
        }
        let _ = session.child.kill();
        let _ = session.child.wait();
        Ok(())
    })
    .await
    .map_err(|error| format!("Could not cancel Windows recording: {error}"))?
}

#[cfg(target_os = "windows")]
fn wait_for_process(
    child: &mut std::process::Child,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("FFmpeg exited unsuccessfully ({status})")),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("FFmpeg did not finalize the recording within 20 seconds; the partial output was removed".into());
            }
            Err(error) => return Err(format!("Could not wait for FFmpeg to finish: {error}")),
        }
    }
}

fn recording_result(selected: SelectedFile, provider: &str, capture_mode: &str) -> ToolResult {
    ToolResult {
        tool_id: "arcade.screen.recorder".into(),
        status: ResultStatus::Success,
        outputs: vec![ToolValue {
            kind: ValueKind::Artifact,
            value: selected.token,
            mime: selected.mime,
        }],
        message: Some("Screen recording finalized and saved as an MP4 video.".into()),
        warnings: vec![],
        metadata: BTreeMap::from([
            ("outputName".into(), json!(selected.name)),
            ("outputBytes".into(), json!(selected.size)),
            ("providerPath".into(), json!(provider)),
            ("captureMode".into(), json!(capture_mode)),
        ]),
    }
}

#[cfg(target_os = "linux")]
fn probe_gstreamer_provider() -> Result<GStreamerProvider, String> {
    let launch = resolve_executable("gst-launch-1.0")?;
    let inspect = resolve_executable("gst-inspect-1.0")?;
    let output = std::process::Command::new(&launch)
        .arg("--version")
        .env_remove("GST_PLUGIN_PATH")
        .env_remove("GST_PLUGIN_PATH_1_0")
        .env_remove("GST_PLUGIN_SYSTEM_PATH")
        .env_remove("GST_PLUGIN_SYSTEM_PATH_1_0")
        .output()
        .map_err(|error| format!("Could not check GStreamer at {}: {error}", launch.display()))?;
    if !output.status.success() {
        return Err(format!(
            "GStreamer version probe failed at {}",
            launch.display()
        ));
    }
    let version_output = String::from_utf8_lossy(&output.stdout);
    let version = version_output
        .lines()
        .find(|line| line.trim_start().starts_with("GStreamer "))
        .map(str::trim)
        .ok_or_else(|| {
            "The discovered gst-launch executable did not identify as GStreamer".to_owned()
        })?
        .to_owned();
    for element in ["pipewiresrc", "videoconvert", "vp8enc", "webmmux", "fdsink"] {
        let exists = std::process::Command::new(&inspect)
            .arg("--exists")
            .arg(element)
            .env_remove("GST_PLUGIN_PATH")
            .env_remove("GST_PLUGIN_PATH_1_0")
            .env_remove("GST_PLUGIN_SYSTEM_PATH")
            .env_remove("GST_PLUGIN_SYSTEM_PATH_1_0")
            .status()
            .map_err(|error| format!("Could not check GStreamer element {element}: {error}"))?;
        if !exists.success() {
            return Err(format!("GStreamer element {element} is unavailable"));
        }
    }
    Ok(GStreamerProvider { launch, version })
}

#[cfg(target_os = "linux")]
fn resolve_executable(name: &str) -> Result<PathBuf, String> {
    let path = std::env::var_os("PATH").ok_or("PATH is not set")?;
    for directory in std::env::split_paths(&path) {
        if directory.as_os_str().is_empty() {
            continue;
        }
        let candidate = directory.join(name);
        let Ok(resolved) = candidate.canonicalize() else {
            continue;
        };
        let Ok(metadata) = fs::metadata(&resolved) else {
            continue;
        };
        use std::os::unix::fs::PermissionsExt;
        if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 {
            return Ok(resolved);
        }
    }
    Err(format!("Could not find compatible {name} on PATH"))
}

#[cfg(target_os = "linux")]
async fn start_linux_recording(
    runtime: std::sync::Arc<Arcade>,
    job: arcade_core::jobs::ExternalJobHandle,
) -> Result<Option<LinuxRecordingSession>, String> {
    use ashpd::desktop::{
        PersistMode,
        screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
    };

    let provider = tauri::async_runtime::spawn_blocking(probe_gstreamer_provider)
        .await
        .map_err(|error| format!("GStreamer capability check stopped unexpectedly: {error}"))??;
    let portal = Screencast::new().await.map_err(|error| {
        format!("The XDG ScreenCast portal is unavailable. Check the desktop portal and PipeWire service. Details: {error}")
    })?;
    let portal_session = portal
        .create_session(Default::default())
        .await
        .map_err(|error| format!("Could not create a ScreenCast session: {error}"))?;
    let source_types = SourceType::Monitor | SourceType::Window;
    if let Err(error) = portal
        .select_sources(
            &portal_session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Embedded)
                .set_sources(source_types)
                .set_multiple(false)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await
        .and_then(|request| request.response().map(|_| ()))
    {
        let _ = portal_session.close().await;
        if is_portal_cancel(&error) {
            return Ok(None);
        }
        return Err(format!("Could not select a screen or window: {error}"));
    }
    let stream_response = match portal
        .start(&portal_session, None, Default::default())
        .await
    {
        Ok(request) => match request.response() {
            Ok(streams) => streams,
            Err(error) if is_portal_cancel(&error) => {
                let _ = portal_session.close().await;
                return Ok(None);
            }
            Err(error) => {
                let _ = portal_session.close().await;
                return Err(format!("ScreenCast selection was not completed: {error}"));
            }
        },
        Err(error) if is_portal_cancel(&error) => {
            let _ = portal_session.close().await;
            return Ok(None);
        }
        Err(error) => {
            let _ = portal_session.close().await;
            return Err(format!("Could not start the ScreenCast session: {error}"));
        }
    };
    let Some(stream) = stream_response.streams().first() else {
        let _ = portal_session.close().await;
        return Err("The ScreenCast portal returned no selected stream".into());
    };
    let node_id = stream.pipe_wire_node_id();
    if node_id == 0 {
        let _ = portal_session.close().await;
        return Err("The ScreenCast portal returned an invalid PipeWire stream".into());
    }
    let remote = match portal
        .open_pipe_wire_remote(&portal_session, Default::default())
        .await
    {
        Ok(remote) => remote,
        Err(error) => {
            let _ = portal_session.close().await;
            return Err(format!(
                "Could not connect to the selected PipeWire stream: {error}"
            ));
        }
    };
    let staging = tempfile::Builder::new()
        .prefix("arcade-screen-recording-")
        .tempdir_in(runtime.artifact_staging_root())
        .map_err(|error| format!("Could not create a private recording workspace: {error}"))?;
    let output_path = staging.path().join("screen-recording.webm");
    let file = create_private_output(&output_path)?;
    let (child, provider) = tauri::async_runtime::spawn_blocking(move || {
        start_gstreamer_process(&provider.launch, node_id, remote, file)
            .map(|child| (child, provider))
    })
    .await
    .map_err(|error| format!("The GStreamer process could not start: {error}"))??;
    Ok(Some(LinuxRecordingSession {
        portal_session: Some(portal_session),
        child,
        _staging: staging,
        output_path,
        provider_path: provider.launch,
        provider_version: provider.version,
        started_at: std::time::Instant::now(),
        job,
    }))
}

#[cfg(target_os = "linux")]
fn create_private_output(path: &std::path::Path) -> Result<fs::File, String> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("Could not create private recording output: {error}"))
}

#[cfg(target_os = "linux")]
fn duplicate_fd_for_exec(fd: std::os::fd::RawFd) -> Result<std::os::fd::OwnedFd, String> {
    use std::os::fd::FromRawFd;
    let duplicated = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 32) };
    if duplicated < 0 {
        return Err(format!(
            "Could not prepare a scoped recording handle: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(duplicated) })
}

#[cfg(target_os = "linux")]
fn start_gstreamer_process(
    launch: &std::path::Path,
    node_id: u32,
    remote: std::os::fd::OwnedFd,
    output: fs::File,
) -> Result<std::process::Child, String> {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    let remote_for_child = duplicate_fd_for_exec(remote.as_raw_fd())?;
    let output_for_child = duplicate_fd_for_exec(output.as_raw_fd())?;
    let remote_fd = remote_for_child.as_raw_fd();
    let output_fd = output_for_child.as_raw_fd();
    let mut command = std::process::Command::new(launch);
    command
        .args(["-e", "pipewiresrc", "fd=3"])
        .arg(format!("path={node_id}"))
        .args([
            "do-timestamp=true",
            "!",
            "queue",
            "max-size-buffers=12",
            "max-size-time=0",
            "max-size-bytes=0",
            "leaky=downstream",
            "!",
            "videoconvert",
            "!",
            "video/x-raw,framerate=30/1",
            "!",
            "vp8enc",
            "deadline=1",
            "cpu-used=8",
            "threads=2",
            "target-bitrate=4000000",
            "keyframe-max-dist=60",
            "!",
            "webmmux",
            "!",
            "fdsink",
            "fd=4",
            "sync=false",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .env_remove("GST_PLUGIN_PATH")
        .env_remove("GST_PLUGIN_PATH_1_0")
        .env_remove("GST_PLUGIN_SYSTEM_PATH")
        .env_remove("GST_PLUGIN_SYSTEM_PATH_1_0");
    // FD sources are private duplicates. dup2 in the child makes only the two
    // explicitly granted handles inheritable by the encoder process.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(remote_fd, 3) < 0 || libc::dup2(output_fd, 4) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start GStreamer at {}: {error}", launch.display()))?;
    drop(remote_for_child);
    drop(output_for_child);
    drop(remote);
    drop(output);
    std::thread::sleep(std::time::Duration::from_millis(250));
    if let Some(status) = child
        .try_wait()
        .map_err(|error| format!("Could not verify the recording pipeline: {error}"))?
    {
        return Err(format!(
            "GStreamer stopped during startup (status {status})."
        ));
    }
    Ok(child)
}

#[cfg(target_os = "linux")]
fn is_portal_cancel(error: &impl std::fmt::Display) -> bool {
    let value = error.to_string().to_ascii_lowercase();
    value.contains("cancel") || value.contains("dismiss")
}

#[cfg(target_os = "linux")]
async fn stop_linux_recording(
    mut session: LinuxRecordingSession,
    runtime: std::sync::Arc<Arcade>,
) -> Result<ToolResult, String> {
    let portal_session = session
        .portal_session
        .take()
        .ok_or("The ScreenCast portal session is already closed")?;
    let finalization = tauri::async_runtime::spawn_blocking(move || {
        let result = finish_gstreamer_process(&mut session.child);
        if let Err(error) = result {
            return Err(error);
        }
        let metadata = fs::metadata(&session.output_path)
            .map_err(|error| format!("The recording output was not created: {error}"))?;
        if metadata.len() < 512 {
            return Err("The recording ended before a valid video was produced".into());
        }
        let selected = runtime
            .publish_staged_output(
                None,
                &session.output_path,
                "screen-recording.webm",
                &AtomicBool::new(false),
            )
            .map_err(|error| format!("Could not keep the screen recording safely: {error}"))?;
        let mut result = recording_result(
            selected,
            &session.provider_path.display().to_string(),
            "xdg-portal-pipewire",
        );
        result
            .metadata
            .insert("providerVersion".into(), json!(session.provider_version));
        result.message = Some("Screen recording finalized as WebM.".into());
        Ok(result)
    })
    .await
    .map_err(|error| format!("Screen recording finalization stopped unexpectedly: {error}"))?;
    let close_result = portal_session.close().await;
    if let Err(error) = close_result {
        if finalization.is_ok() {
            return Err(format!(
                "The video was saved, but the portal session did not close cleanly: {error}"
            ));
        }
    }
    finalization
}

#[cfg(target_os = "linux")]
fn finish_gstreamer_process(child: &mut std::process::Child) -> Result<(), String> {
    use std::os::unix::process::ExitStatusExt;
    if child
        .try_wait()
        .map_err(|error| error.to_string())?
        .is_none()
    {
        let pid = i32::try_from(child.id()).map_err(|_| "GStreamer process ID is out of range")?;
        if unsafe { libc::kill(pid, libc::SIGINT) } != 0 {
            return Err(format!(
                "Could not request a clean WebM finalization: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("GStreamer did not finalize the recording within 12 seconds; the partial file was removed".into());
            }
            Err(error) => return Err(format!("Could not wait for GStreamer to finish: {error}")),
        }
    };
    if !status.success() && status.signal() != Some(libc::SIGINT) {
        return Err(format!(
            "GStreamer exited unsuccessfully while finalizing the recording ({status})"
        ));
    }
    Ok(())
}

pub async fn run_screen_tool(
    tool_id: &str,
    runtime: std::sync::Arc<Arcade>,
    cancelled: std::sync::Arc<AtomicBool>,
) -> Result<Option<ToolResult>, String> {
    if !matches!(
        tool_id,
        "arcade.screen.screenshot"
            | "arcade.screen.qr"
            | "arcade.screen.ocr"
            | "arcade.screen.color"
            | "arcade.screen.ruler"
            | "arcade.screen.pin"
    ) {
        return Err("This screen action is not available yet".into());
    }
    use arcade_core::link::consumer;
    use arcade_link::{InvokeRequest, ids};
    let mode = match tool_id {
        "arcade.screen.ruler" => Some("measure"),
        "arcade.screen.pin" => Some("pin"),
        "arcade.screen.color" => Some("color"),
        _ => None,
    };
    if let Some(mode) = mode
        .filter(|_| consumer::peer_action(&runtime, ids::LENS, "lens.capture_and_act").is_some())
    {
        let worker = runtime.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            consumer::invoke(
                &worker,
                ids::LENS,
                InvokeRequest::new("lens.capture_and_act", ids::BOX).options(json!({"mode":mode})),
                &cancelled,
            )
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.user_message("Arcade Lens"))?;
        return Ok(Some(ToolResult {
            tool_id: tool_id.into(),
            status: ResultStatus::Success,
            outputs: vec![],
            message: result.message,
            warnings: vec![],
            metadata: BTreeMap::from([
                ("providerId".into(), json!("screen.select.lens")),
                ("handedOver".into(), json!(true)),
            ]),
        }));
    }
    let delegated = consumer::peer_action(&runtime, ids::LENS, "lens.capture").is_some();
    let selected = if delegated {
        let worker = runtime.clone();
        let cancel = cancelled.clone();
        let selected = tauri::async_runtime::spawn_blocking(move || {
            consumer::capture_region(&worker, &cancel)
        })
        .await
        .map_err(|e| e.to_string())??;
        let path = runtime
            .grants()
            .resolve(&selected.token)
            .map_err(|e| e.to_string())?;
        let validated = validate_png_screenshot(&path);
        let published = validated.and_then(|_| {
            runtime
                .publish_staged_output(None, &path, "screen-selection.png", &cancelled)
                .map_err(|e| e.to_string())
        });
        runtime.grants().revoke(&selected.token);
        let _ = fs::remove_file(path);
        published?
    } else {
        let Some(selected) = capture_screen_area(&runtime).await? else {
            return Ok(None);
        };
        selected
    };

    if tool_id == "arcade.screen.screenshot"
        || tool_id == "arcade.screen.color"
        || tool_id == "arcade.screen.ruler"
        || tool_id == "arcade.screen.pin"
    {
        return Ok(Some(ToolResult {
            tool_id: tool_id.into(),
            status: ResultStatus::Success,
            outputs: vec![ToolValue {
                kind: ValueKind::Artifact,
                value: selected.token,
                mime: selected.mime,
            }],
            message: Some(if tool_id == "arcade.screen.color" {
                "Captured a local screen image for exact pixel sampling.".into()
            } else if tool_id == "arcade.screen.ruler" {
                "Captured a local screen image for pixel measurement.".into()
            } else {
                "Captured screen content using the system selection UI.".into()
            }),
            warnings: vec![],
            metadata: BTreeMap::from([
                ("outputName".into(), json!(selected.name)),
                ("outputBytes".into(), json!(selected.size)),
                ("captureMode".into(), json!("user-selected")),
                (
                    "selectionProvider".into(),
                    json!(if delegated {
                        "screen.select.lens"
                    } else {
                        "native"
                    }),
                ),
            ]),
        }));
    }

    let runtime = runtime.clone();
    let target_tool_id = match tool_id {
        "arcade.screen.qr" => "arcade.barcode.decode",
        "arcade.screen.ocr" => "arcade.image.ocr",
        _ => unreachable!("screenshot and color capture handled above"),
    };
    let request = ToolRequest {
        tool_id: target_tool_id.into(),
        inputs: vec![ToolValue {
            kind: ValueKind::Artifact,
            value: selected.token,
            mime: selected.mime,
        }],
        options: json!({}),
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        runtime.run_tool_with_cancel(request, &cancelled)
    })
    .await
    .map_err(|error| format!("Screen QR scan stopped unexpectedly: {error}"))?
    .map_err(|error| format!("Could not scan the selected screen content: {error}"))?;
    let mut result = result;
    result.tool_id = tool_id.into();
    result.metadata.insert(
        "selectionProvider".into(),
        json!(if delegated {
            "screen.select.lens"
        } else {
            "native"
        }),
    );
    Ok(Some(result))
}

#[cfg(target_os = "linux")]
async fn capture_linux_area(runtime: &Arcade) -> Result<Option<SelectedFile>, String> {
    let portal_result = capture_linux_portal_area(runtime).await;
    match portal_result {
        Ok(result) => Ok(result),
        Err(portal_error) => {
            let Some(executable) = linux_x11_import_executable() else {
                return Err(portal_error);
            };
            capture_linux_x11_area(runtime, executable).await
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_x11_import_executable() -> Option<PathBuf> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_none() {
        return None;
    }
    arcade_core::provider::find_system_executable("import")
}

#[cfg(target_os = "linux")]
async fn capture_linux_portal_area(runtime: &Arcade) -> Result<Option<SelectedFile>, String> {
    use ashpd::desktop::screenshot::{AvailableTargets, Screenshot, ScreenshotProxy};

    let portal = ScreenshotProxy::new().await.map_err(|error| {
        format!(
            "The XDG Screenshot portal is unavailable. Check that your desktop portal service is running. Details: {error}"
        )
    })?;
    let targets = portal.available_targets().await.map_err(|error| {
        format!(
            "This desktop's screenshot portal cannot report supported capture targets. Area selection needs Screenshot portal version 3 or later. Details: {error}"
        )
    })?;
    if !targets.contains(AvailableTargets::Area) {
        return Err(
            "This desktop portal does not support selecting a screen area. You can still choose an image file.".into(),
        );
    }

    let request = Screenshot::request()
        .interactive(true)
        .modal(true)
        .target(AvailableTargets::Area)
        .send()
        .await
        .map_err(|error| format!("Could not open the system area selector: {error}"))?;
    let response = match request.response() {
        Ok(response) => response,
        Err(error) if error.to_string().to_ascii_lowercase().contains("cancel") => return Ok(None),
        Err(error) => return Err(format!("Screen selection was not completed: {error}")),
    };
    let path = local_screenshot_path(&response.uri().to_string())?;
    validate_png_screenshot(&path)?;

    runtime
        .publish_staged_output(None, &path, "screen-selection.png", &AtomicBool::new(false))
        .map(Some)
        .map_err(|error| format!("Could not keep the selected screen image safely: {error}"))
}

#[cfg(target_os = "linux")]
async fn capture_linux_x11_area(
    runtime: &Arcade,
    executable: PathBuf,
) -> Result<Option<SelectedFile>, String> {
    use std::process::{Command, Stdio};

    let staging = staging_directory(runtime)?;
    let output_path = staging.path().join("screen-selection.png");
    let command_path = output_path.clone();
    let output = tauri::async_runtime::spawn_blocking(move || {
        Command::new(executable)
            .arg(&command_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
    })
    .await
    .map_err(|error| format!("The X11 area selector stopped unexpectedly: {error}"))?
    .map_err(|error| format!("Could not start the X11 area selector: {error}"))?;

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if detail.to_ascii_lowercase().contains("cancel") && !output_path.exists() {
            return Ok(None);
        }
        return Err(if detail.is_empty() {
            format!("The X11 area selector exited with {}.", output.status)
        } else {
            format!("The X11 area selector failed: {detail}")
        });
    }
    if !output_path.exists() {
        // ImageMagick's right-click cancellation exits without creating a file.
        return Ok(None);
    }
    publish_capture(runtime, &output_path)
}

#[cfg(target_os = "windows")]
async fn capture_windows_area(runtime: &Arcade) -> Result<Option<SelectedFile>, String> {
    let runtime = runtime.clone();
    tauri::async_runtime::spawn_blocking(move || capture_windows_frame(&runtime))
        .await
        .map_err(|error| format!("Windows screen capture stopped unexpectedly: {error}"))?
}

#[cfg(target_os = "macos")]
async fn capture_macos_area(runtime: &Arcade) -> Result<Option<SelectedFile>, String> {
    use screencapturekit::{
        async_api::{AsyncSCContentSharingPicker, AsyncSCScreenshotManager},
        content_sharing_picker::{
            SCContentSharingPicker, SCContentSharingPickerConfiguration, SCPickerOutcome,
        },
        screenshot_manager::{CGImageExt, ImageFormat},
        stream::configuration::SCStreamConfiguration,
    };

    if !SCContentSharingPicker::is_available() {
        return Err("The macOS content picker is unavailable. This capture flow requires macOS 14 or later.".into());
    }

    let picker_config = SCContentSharingPickerConfiguration::new();
    let selected = match AsyncSCContentSharingPicker::show(&picker_config).await {
        SCPickerOutcome::Picked(selected) => selected,
        SCPickerOutcome::Cancelled => return Ok(None),
        SCPickerOutcome::Error(error) => {
            return Err(format!("Could not open the macOS screen picker: {error}"));
        }
    };

    let (width, height) = selected.pixel_size();
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width == 0 || height == 0 || pixels > MAX_CAPTURE_PIXELS {
        return Err("The selected screen or window is too large to capture safely".into());
    }

    let filter = selected.filter();
    let config = SCStreamConfiguration::new()
        .with_width(width)
        .with_height(height);
    let image = AsyncSCScreenshotManager::capture_image(&filter, &config)
        .await
        .map_err(|error| {
            format!(
                "ScreenCaptureKit could not capture the selected content. Check Screen Recording permission in System Settings. Details: {error}"
            )
        })?;

    let staging = staging_directory(runtime)?;
    let output_path = staging.path().join("screen-selection.png");
    image
        .save(
            output_path
                .to_str()
                .ok_or_else(|| "The private screenshot path is not valid Unicode".to_owned())?,
            ImageFormat::Png,
        )
        .map_err(|error| format!("Could not encode the selected screen image: {error}"))?;
    publish_capture(runtime, &output_path)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
async fn capture_unsupported_area(_runtime: &Arcade) -> Result<Option<SelectedFile>, String> {
    Err(
        "Screen capture is not supported on this platform build. Choose an image file instead."
            .into(),
    )
}

#[cfg(target_os = "windows")]
fn capture_windows_frame(runtime: &Arcade) -> Result<Option<SelectedFile>, String> {
    use windows_capture::{
        capture::{Context, GraphicsCaptureApiHandler},
        encoder::ImageFormat,
        frame::Frame,
        graphics_capture_api::InternalCaptureControl,
        graphics_capture_picker::GraphicsCapturePicker,
        settings::{
            ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
            MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
        },
    };

    let Some(selected) = GraphicsCapturePicker::pick_item()
        .map_err(|error| format!("Could not open the Windows capture picker: {error}"))?
    else {
        return Ok(None);
    };
    let (width, height) = selected
        .size()
        .map_err(|error| format!("Could not inspect the selected capture target: {error}"))?;
    if width <= 0 || height <= 0 || checked_dimensions(width as u32, height as u32).is_err() {
        return Err("The selected screen or window is too large to capture safely".into());
    }

    let staging = staging_directory(runtime)?;
    let output_path = staging.path().join("screen-selection.png");
    struct OneFrameCapture {
        output_path: PathBuf,
        captured: bool,
    }
    impl GraphicsCaptureApiHandler for OneFrameCapture {
        type Flags = PathBuf;
        type Error = Box<dyn std::error::Error + Send + Sync>;

        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            Ok(Self {
                output_path: ctx.flags,
                captured: false,
            })
        }

        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame<'_>,
            capture_control: InternalCaptureControl,
        ) -> Result<(), Self::Error> {
            if !self.captured {
                frame.save_as_image(&self.output_path, ImageFormat::Png)?;
                self.captured = true;
            }
            capture_control.stop();
            Ok(())
        }
    }

    let settings = Settings::new(
        selected,
        CursorCaptureSettings::Default,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        output_path.clone(),
    );
    OneFrameCapture::start(settings).map_err(|error| {
        format!("Windows Graphics Capture could not capture this target: {error}")
    })?;
    if !output_path.is_file() {
        return Err("Windows capture ended before a frame was delivered".into());
    }
    publish_capture(runtime, &output_path)
}

fn local_screenshot_path(uri: &str) -> Result<PathBuf, String> {
    let parsed = url::Url::parse(uri)
        .map_err(|_| "The screenshot portal returned an invalid URI".to_owned())?;
    if parsed.scheme() != "file"
        || parsed
            .host_str()
            .is_some_and(|host| !host.is_empty() && !host.eq_ignore_ascii_case("localhost"))
    {
        return Err("The screenshot portal returned a non-local file URI".into());
    }
    parsed
        .to_file_path()
        .map_err(|_| "The screenshot portal returned an invalid local file path".to_owned())
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn staging_directory(runtime: &Arcade) -> Result<tempfile::TempDir, String> {
    tempfile::Builder::new()
        .prefix("arcade-screen-capture-")
        .tempdir_in(runtime.artifact_staging_root())
        .map_err(|error| format!("Could not create a private capture workspace: {error}"))
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
fn publish_capture(
    runtime: &Arcade,
    path: &std::path::Path,
) -> Result<Option<SelectedFile>, String> {
    validate_png_screenshot(path)?;
    runtime
        .publish_staged_output(None, path, "screen-selection.png", &AtomicBool::new(false))
        .map(Some)
        .map_err(|error| format!("Could not keep the selected screen image safely: {error}"))
}

fn validate_png_screenshot(path: &std::path::Path) -> Result<(), String> {
    let link_metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("The selected screen image is unavailable: {error}"))?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        return Err("The screen selector did not return a regular image file".into());
    }
    if link_metadata.len() == 0 || link_metadata.len() > MAX_CAPTURE_BYTES {
        return Err("The selected screen image is empty or larger than 128 MB".into());
    }
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut header = [0u8; 24];
    file.read_exact(&mut header)
        .map_err(|_| "The screen selector returned a truncated image".to_owned())?;
    if &header[..8] != PNG_SIGNATURE || &header[12..16] != b"IHDR" {
        return Err(
            "The screen selector returned an unsupported image format; Arcade Box expected PNG"
                .into(),
        );
    }
    let width = u32::from_be_bytes(header[16..20].try_into().expect("fixed PNG header slice"));
    let height = u32::from_be_bytes(header[20..24].try_into().expect("fixed PNG header slice"));
    checked_dimensions(width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_uri_must_be_local_file_uri() {
        assert!(local_screenshot_path("file:///home/user/screen.png").is_ok());
        assert!(local_screenshot_path("file://localhost/home/user/screen.png").is_ok());
        assert!(local_screenshot_path("https://example.test/screen.png").is_err());
        assert!(local_screenshot_path("file://remote-host/home/user/screen.png").is_err());
    }

    #[test]
    fn capture_dimensions_reject_empty_and_oversized_content() {
        assert!(checked_dimensions(1, 1).is_ok());
        assert!(checked_dimensions(0, 1).is_err());
        assert!(checked_dimensions(32_768, 32_768).is_err());
        assert!(checked_dimensions(32_769, 1).is_err());
    }
}

fn checked_dimensions(width: u32, height: u32) -> Result<(), String> {
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width == 0 || height == 0 || width > 32_768 || height > 32_768 || pixels > MAX_CAPTURE_PIXELS
    {
        return Err("The selected screen image dimensions exceed the safe capture limit".into());
    }
    Ok(())
}
