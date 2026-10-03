//! Previews for the media editors: video frames and size estimates, audio
//! waveforms, detected silence, cover art, and loudness.
//!
//! The webview only receives small JPEG images and numbers for a granted file
//! token, never a filesystem path. Work runs off the UI thread and is limited
//! so scrubbing can't queue many FFmpeg processes.

use std::sync::{Arc, LazyLock, atomic::AtomicBool};
use tauri::State;
use tokio::sync::Semaphore;

static MEDIA_WORK: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));

#[tauri::command]
pub async fn video_preview(
    token: String,
    thumbnails: Option<u32>,
    frame_at: Option<f64>,
    runtime: State<'_, Arc<arcade_core::Arcade>>,
) -> Result<arcade_core::media::VideoPreview, String> {
    // A negative time asks for the middle of the clip.
    if frame_at.is_some_and(|time| !time.is_finite()) {
        return Err("Invalid preview time".into());
    }
    let runtime = runtime.inner().clone();
    let permit = MEDIA_WORK
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| format!("Video preview service is unavailable: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        arcade_core::media::video_preview(
            runtime.grants(),
            &token,
            thumbnails.unwrap_or(0),
            frame_at,
            &AtomicBool::new(false),
        )
    })
    .await
    .map_err(|error| format!("Video preview failed: {error}"))?
}

#[tauri::command]
pub async fn estimate_video_output(
    token: String,
    options: serde_json::Value,
    runtime: State<'_, Arc<arcade_core::Arcade>>,
) -> Result<arcade_core::media::VideoEstimate, String> {
    if !options.is_object() {
        return Err("Estimate options must be an object".into());
    }
    let runtime = runtime.inner().clone();
    let permit = MEDIA_WORK
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| format!("Video estimate service is unavailable: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        arcade_core::media::estimate_video_output(
            runtime.grants(),
            &token,
            options,
            &AtomicBool::new(false),
        )
    })
    .await
    .map_err(|error| format!("Video estimate failed: {error}"))?
}

#[tauri::command]
pub async fn audio_preview(
    token: String,
    columns: Option<u32>,
    silence_threshold_db: Option<f64>,
    silence_minimum_seconds: Option<f64>,
    runtime: State<'_, Arc<arcade_core::Arcade>>,
) -> Result<arcade_core::media::AudioPreview, String> {
    let silence = match (silence_threshold_db, silence_minimum_seconds) {
        (Some(threshold), Some(minimum)) if threshold.is_finite() && minimum.is_finite() => {
            Some((threshold, minimum))
        }
        (None, None) => None,
        _ => return Err("Invalid silence settings".into()),
    };
    let runtime = runtime.inner().clone();
    let permit = MEDIA_WORK
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| format!("Audio preview service is unavailable: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        arcade_core::media::audio_preview(
            runtime.grants(),
            &token,
            columns.unwrap_or(0),
            silence,
            &AtomicBool::new(false),
        )
    })
    .await
    .map_err(|error| format!("Audio preview failed: {error}"))?
}

#[tauri::command]
pub async fn measure_audio_loudness(
    token: String,
    runtime: State<'_, Arc<arcade_core::Arcade>>,
) -> Result<arcade_core::media::LoudnessReport, String> {
    let runtime = runtime.inner().clone();
    let permit = MEDIA_WORK
        .clone()
        .acquire_owned()
        .await
        .map_err(|error| format!("Loudness measurement is unavailable: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        arcade_core::media::measure_audio_loudness(
            runtime.grants(),
            &token,
            &AtomicBool::new(false),
        )
    })
    .await
    .map_err(|error| format!("Loudness measurement failed: {error}"))?
}
