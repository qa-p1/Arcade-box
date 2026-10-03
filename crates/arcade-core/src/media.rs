//! FFmpeg-backed video and audio actions.
//!
//! All provider calls use argument vectors, private staging directories, typed
//! file grants, cancellation, and create-if-absent publication. UI options are
//! validated here as untrusted data even when a standard form supplied them.

use crate::{
    artifacts::{publish_without_overwrite, validate_portable_filename},
    grants::{FileGrants, SelectedFile},
    process::{self, ProcessSpec},
    provider::{ProviderInfo, discover_ffmpeg},
};
use arcade_contract::{ResultStatus, ToolManifest, ToolRequest, ToolResult, ToolValue, ValueKind};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

mod audio;
mod job;
mod speech;
mod video;

pub use audio::{AudioPreview, LoudnessReport, audio_preview, measure_audio_loudness};
pub use speech::text_to_speech;
pub(crate) use speech::transcribe_to_segments;
pub use video::{
    PreviewFrame, StreamSummary, VideoEstimate, VideoPreview, estimate_video_output, video_preview,
};

const MAX_INPUTS: usize = 100;
/// Tools that send audio to the speech service rather than only FFmpeg.
const SPEECH_TOOLS: [&str; 2] = ["arcade.audio.transcribe", "arcade.video.auto-subtitles"];
const PROCESS_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Copy)]
struct Format {
    extension: &'static str,
    muxer: &'static str,
    video_codec: Option<&'static str>,
    audio_codec: Option<&'static str>,
    audio_bitrate: Option<&'static str>,
}

pub fn execute(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    execute_with_progress(manifest, request, grants, cancelled, Arc::new(|_| {}))
}

pub fn execute_with_progress(
    manifest: &ToolManifest,
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
    progress: Arc<dyn Fn(f64) + Send + Sync>,
) -> Result<ToolResult, String> {
    let paths = resolve_inputs(request, grants)?;
    validate_input_count(manifest, request)?;
    let required = required_capabilities(manifest, request, &paths)?;
    let provider = discover_ffmpeg(None)
        .into_iter()
        .find(|provider| {
            provider.compatible
                && required
                    .iter()
                    .all(|cap| provider.capabilities.iter().any(|have| have == cap))
        })
        .ok_or_else(|| provider_error(&required))?;

    if SPEECH_TOOLS.contains(&manifest.id.as_str()) {
        let job = job::Job {
            manifest,
            provider: &provider,
            request,
            grants,
            cancelled,
            progress,
        };
        return if manifest.id == "arcade.audio.transcribe" {
            speech::transcribe(&job, &paths[0])
        } else {
            speech::auto_subtitles(&job, &paths[0])
        };
    }
    match manifest.id.as_str() {
        "arcade.video.inspect" => inspect(manifest, &provider, &paths[0], cancelled),
        id if id.starts_with("arcade.video.") => video::execute(
            manifest, &provider, &paths, request, grants, cancelled, progress,
        ),
        id if id.starts_with("arcade.audio.") => audio::execute(
            manifest, &provider, &paths, request, grants, cancelled, progress,
        ),
        _ => Err(format!(
            "No media executor is registered for {}",
            manifest.id
        )),
    }
}

fn resolve_inputs(request: &ToolRequest, grants: &FileGrants) -> Result<Vec<PathBuf>, String> {
    if request.inputs.is_empty() || request.inputs.len() > MAX_INPUTS {
        return Err(format!("Select between 1 and {MAX_INPUTS} media files"));
    }
    request
        .inputs
        .iter()
        .map(|input| {
            if input.kind != ValueKind::Artifact {
                return Err("Select source files through Arcade Box".into());
            }
            grants
                .resolve(&input.value)
                .map_err(|error| error.to_string())
        })
        .collect()
}

fn validate_input_count(manifest: &ToolManifest, request: &ToolRequest) -> Result<(), String> {
    let count = request.inputs.len();
    match manifest.id.as_str() {
        "arcade.video.inspect" if count == 1 => Ok(()),
        id if SPEECH_TOOLS.contains(&id) => {
            if count == 1 {
                Ok(())
            } else {
                Err("Choose one file".into())
            }
        }
        id if id.starts_with("arcade.video.") && id != "arcade.video.inspect" => {
            video::validate_count(id, request, count)
        }
        id if id.starts_with("arcade.audio.") => audio::validate_count(id, request, count),
        _ if count == 1 => Ok(()),
        _ => Err(format!("{} takes exactly one selected file", manifest.name)),
    }
}

fn required_capabilities(
    manifest: &ToolManifest,
    request: &ToolRequest,
    paths: &[PathBuf],
) -> Result<Vec<String>, String> {
    let mut required = vec!["probe:streams".to_string()];
    let id = manifest.id.as_str();
    match id {
        "arcade.video.inspect" => {}
        "arcade.audio.transcribe" => required.push("encoder:libmp3lame".into()),
        "arcade.video.auto-subtitles" => {
            required.push("encoder:libmp3lame".into());
            match request
                .options
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("embed")
            {
                "burn" => required.extend([
                    "filter:subtitles".into(),
                    "encoder:libx264".into(),
                    "encoder:aac".into(),
                ]),
                "embed" => required.push("encoder:mov_text".into()),
                _ => {}
            }
        }
        id if id.starts_with("arcade.video.") => {
            video::required(id, request, paths, &mut required)?
        }
        id if id.starts_with("arcade.audio.") => audio::required(id, request, &mut required)?,
        _ => return Err(format!("No media executor is registered for {id}")),
    }
    required.sort();
    required.dedup();
    Ok(required)
}

fn add_format_capabilities(required: &mut Vec<String>, format: Format, video: bool) {
    if video && let Some(codec) = format.video_codec {
        required.push(format!("encoder:{codec}"));
    }
    if let Some(codec) = format.audio_codec {
        required.push(format!("encoder:{codec}"));
    }
    required.push(format!("mux:{}", format.muxer));
}

fn provider_error(required: &[String]) -> String {
    format!(
        "No compatible system FFmpeg/ffprobe provides the required capabilities: {}. Install or select a compatible FFmpeg provider in Engines & Dependencies.",
        required.join(", ")
    )
}

fn audio_format(value: &str) -> Result<Format, String> {
    match value {
        "mp3" => Ok(Format {
            extension: "mp3",
            muxer: "mp3",
            video_codec: None,
            audio_codec: Some("libmp3lame"),
            audio_bitrate: Some("192k"),
        }),
        "wav" => Ok(Format {
            extension: "wav",
            muxer: "wav",
            video_codec: None,
            audio_codec: Some("pcm_s16le"),
            audio_bitrate: None,
        }),
        "flac" => Ok(Format {
            extension: "flac",
            muxer: "flac",
            video_codec: None,
            audio_codec: Some("flac"),
            audio_bitrate: None,
        }),
        "ogg" => Ok(Format {
            extension: "ogg",
            muxer: "ogg",
            video_codec: None,
            audio_codec: Some("libvorbis"),
            audio_bitrate: Some("192k"),
        }),
        "opus" => Ok(Format {
            extension: "opus",
            muxer: "ogg",
            video_codec: None,
            audio_codec: Some("libopus"),
            audio_bitrate: Some("160k"),
        }),
        "m4a" => Ok(Format {
            extension: "m4a",
            muxer: "ipod",
            video_codec: None,
            audio_codec: Some("aac"),
            audio_bitrate: Some("192k"),
        }),
        other => Err(format!("Unsupported audio output format: {other}")),
    }
}

fn option_str<'a>(
    request: &'a ToolRequest,
    key: &str,
    default: &'a str,
) -> Result<&'a str, String> {
    match request.options.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value.as_str().ok_or_else(|| format!("{key} must be text")),
    }
}

fn option_u64(
    request: &ToolRequest,
    key: &str,
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64, String> {
    let value = match request.options.get(key) {
        None | Some(Value::Null) => default,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| format!("{key} must be an integer"))?,
    };
    if !(min..=max).contains(&value) {
        return Err(format!("{key} must be between {min} and {max}"));
    }
    Ok(value)
}

fn option_choice_u64(
    request: &ToolRequest,
    key: &str,
    default: u64,
    allowed: &[u64],
) -> Result<u64, String> {
    let value = match request.options.get(key) {
        None | Some(Value::Null) => default,
        Some(Value::Number(value)) => value
            .as_u64()
            .ok_or_else(|| format!("{key} must be a supported integer"))?,
        Some(Value::String(value)) => value
            .parse::<u64>()
            .map_err(|_| format!("{key} must be a supported integer"))?,
        Some(_) => return Err(format!("{key} must be a supported integer")),
    };
    if !allowed.contains(&value) {
        return Err(format!(
            "{key} must be one of {}",
            allowed
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(value)
}

fn option_f64(
    request: &ToolRequest,
    key: &str,
    default: f64,
    min: f64,
    max: f64,
) -> Result<f64, String> {
    let value = match request.options.get(key) {
        None | Some(Value::Null) => default,
        Some(value) => value
            .as_f64()
            .ok_or_else(|| format!("{key} must be a number"))?,
    };
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(format!("{key} must be between {min} and {max}"));
    }
    Ok(value)
}

fn option_bool(request: &ToolRequest, key: &str, default: bool) -> Result<bool, String> {
    match request.options.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| format!("{key} must be true or false")),
    }
}

fn validate_range(request: &ToolRequest) -> Result<(Option<f64>, Option<f64>), String> {
    let start = if request.options.get("startSeconds").is_some() {
        Some(option_f64(request, "startSeconds", 0.0, 0.0, 7_200_000.0)?)
    } else {
        None
    };
    let end = if request.options.get("endSeconds").is_some() {
        Some(option_f64(request, "endSeconds", 0.0, 0.001, 7_200_000.0)?)
    } else {
        None
    };
    if matches!((start, end), (Some(start), Some(end)) if end <= start) {
        return Err("End time must be later than start time".into());
    }
    Ok((start, end))
}

fn format_seconds(value: f64) -> String {
    format!("{value:.6}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn default_output_name(input: &Path, suffix: &str, extension: &str) -> String {
    let source = input.file_stem().unwrap_or_default().to_string_lossy();
    let mut stem = String::new();
    for character in source.chars() {
        if character.is_control()
            || matches!(
                character,
                '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'
            )
        {
            stem.push('_');
        } else {
            stem.push(character);
        }
        if stem.len() >= 160 {
            break;
        }
    }
    while stem.ends_with('.') || stem.ends_with(' ') {
        stem.pop();
    }
    if stem.is_empty() {
        stem.push_str("media");
    }
    let reserved = stem
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(reserved.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || reserved.starts_with("COM")
        || reserved.starts_with("LPT")
    {
        stem.insert(0, '_');
    }
    if suffix.is_empty() {
        format!("{stem}.{extension}")
    } else {
        format!("{stem}-{suffix}.{extension}")
    }
}

fn sanitize_piece(value: &str) -> String {
    let mut output = String::new();
    for character in value.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
            output.push(character);
        } else if character.is_whitespace() {
            output.push('-');
        } else if !character.is_control() {
            output.push('_');
        }
        if output.len() >= 100 {
            break;
        }
    }
    output.trim_matches(['.', '-', '_']).to_owned()
}

fn requested_output_name(
    request: &ToolRequest,
    input: &Path,
    suffix: &str,
    extension: &str,
) -> Result<String, String> {
    let Some(value) = request.options.get("outputName") else {
        return Ok(default_output_name(input, suffix, extension));
    };
    let name = value.as_str().ok_or("outputName must be text")?;
    let name = if Path::new(name).extension().is_none() {
        format!("{name}.{extension}")
    } else {
        name.to_owned()
    };
    if Path::new(&name).extension().and_then(|ext| ext.to_str()) != Some(extension) {
        return Err(format!("Output name must use the .{extension} extension"));
    }
    validate_portable_filename(&name)?;
    Ok(name)
}

fn stage_directory(input: &Path) -> Result<tempfile::TempDir, String> {
    let parent = input
        .parent()
        .ok_or("Source path has no parent directory")?;
    tempfile::Builder::new()
        .prefix(".arcade-media-")
        .tempdir_in(parent)
        .map_err(|error| format!("Cannot create private output staging folder: {error}"))
}

fn ffprobe_path(ffmpeg: &Path) -> PathBuf {
    ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    })
}

fn ffprobe_json(ffmpeg: &Path, input: &Path, cancelled: &AtomicBool) -> Result<Value, String> {
    let output = process::run(
        &ProcessSpec {
            executable: ffprobe_path(ffmpeg),
            args: vec![
                "-v".into(),
                "error".into(),
                "-show_format".into(),
                "-show_streams".into(),
                "-show_chapters".into(),
                "-of".into(),
                "json".into(),
                input.as_os_str().to_os_string(),
            ],
            current_dir: None,
            timeout: Duration::from_secs(30),
            output_limit: 8 * 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "ffprobe could not inspect the selected file: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("ffprobe returned invalid JSON: {error}"))
}

fn inspect(
    manifest: &ToolManifest,
    provider: &ProviderInfo,
    input: &Path,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let value = ffprobe_json(&provider.executable_path, input, cancelled)?;
    let pretty = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    let mut result = base_result(
        manifest,
        vec![ToolValue::text(pretty, "structured/media-info")],
        "Media details ready",
    );
    result.metadata.insert(
        "providerPath".into(),
        json!(
            ffprobe_path(&provider.executable_path)
                .display()
                .to_string()
        ),
    );
    result
        .metadata
        .insert("providerVersion".into(), json!(provider.version));
    Ok(result)
}

fn progress_seconds(line: &str) -> Option<f64> {
    if let Some(value) = line
        .strip_prefix("out_time_us=")
        .or_else(|| line.strip_prefix("out_time_ms="))
    {
        return value
            .trim()
            .parse::<f64>()
            .ok()
            .map(|microseconds| microseconds / 1_000_000.0);
    }
    let value = line.strip_prefix("out_time=")?.trim();
    let mut pieces = value.split(':');
    let hours = pieces.next()?.parse::<f64>().ok()?;
    let minutes = pieces.next()?.parse::<f64>().ok()?;
    let seconds = pieces.next()?.parse::<f64>().ok()?;
    pieces
        .next()
        .is_none()
        .then_some(hours * 3600.0 + minutes * 60.0 + seconds)
}

/// One FFmpeg input. `options` precede its `-i`, for example an input seek.
struct MediaInput {
    options: Vec<OsString>,
    path: PathBuf,
}

impl MediaInput {
    fn plain(path: &Path) -> Self {
        Self {
            options: Vec::new(),
            path: path.to_path_buf(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn run_ffmpeg(
    ffmpeg: &Path,
    inputs: &[MediaInput],
    args: Vec<OsString>,
    current_dir: &Path,
    duration: Option<f64>,
    cancelled: &AtomicBool,
    progress: Arc<dyn Fn(f64) + Send + Sync>,
    log_level: &str,
) -> Result<process::ProcessOutput, String> {
    let mut full = vec![
        "-nostdin".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        log_level.into(),
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
    ];
    for input in inputs {
        full.extend(input.options.iter().cloned());
        full.push("-i".into());
        full.push(input.path.as_os_str().to_os_string());
    }
    full.extend(args);
    let spec = ProcessSpec {
        executable: ffmpeg.to_path_buf(),
        args: full,
        current_dir: Some(current_dir.to_path_buf()),
        timeout: PROCESS_TIMEOUT,
        output_limit: 2 * 1024 * 1024,
    };
    let reported = progress.clone();
    let output = process::run_with_stdout_lines(
        &spec,
        cancelled,
        Arc::new(move |line| {
            if let (Some(total), Some(elapsed)) = (duration, progress_seconds(line))
                && total > 0.0
            {
                reported((elapsed / total).clamp(0.0, 0.99));
            }
        }),
    )
    .map_err(|error| error.to_string())?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("Operation cancelled".into());
    }
    if !output.status.success() {
        return Err(format!(
            "FFmpeg failed using {}: {}",
            ffmpeg.display(),
            concise_ffmpeg_error(&String::from_utf8_lossy(&output.stderr))
        ));
    }
    Ok(output)
}

/// FFmpeg prints many lines per failure; the last few carry the cause.
fn concise_ffmpeg_error(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let text = lines[lines.len().saturating_sub(4)..].join(" · ");
    if text.chars().count() > 600 {
        text.chars().take(600).collect::<String>() + "…"
    } else {
        text
    }
}

fn optional_integer(
    request: &ToolRequest,
    key: &str,
    min: u64,
    max: u64,
) -> Result<Option<u64>, String> {
    match request.options.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.trim().is_empty() => Ok(None),
        Some(_) => option_u64(request, key, min, min, max).map(Some),
    }
}

fn base_result(manifest: &ToolManifest, outputs: Vec<ToolValue>, message: &str) -> ToolResult {
    ToolResult {
        tool_id: manifest.id.clone(),
        status: ResultStatus::Success,
        outputs,
        message: Some(message.into()),
        warnings: Vec::new(),
        metadata: Default::default(),
    }
}

fn attach_provider(result: &mut ToolResult, provider: &ProviderInfo) {
    result.metadata.insert(
        "providerPath".into(),
        json!(provider.executable_path.display().to_string()),
    );
    result
        .metadata
        .insert("providerVersion".into(), json!(provider.version));
}

fn publish_staged(
    staged: &Path,
    parent: &Path,
    name: &str,
    grants: &FileGrants,
    cancelled: &AtomicBool,
) -> Result<(PathBuf, SelectedFile), String> {
    if !staged.is_file() {
        return Err("FFmpeg exited successfully but did not create an output file".into());
    }
    let final_path = publish_without_overwrite(staged, parent, name, cancelled)
        .map_err(|error| format!("Could not save output: {error}"))?;
    let selected = grants
        .grant(&final_path)
        .map_err(|error| error.to_string())?;
    Ok((final_path, selected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_ffmpeg_runs_basic_video_workflows() {
        let Some(provider) = discover_ffmpeg(None)
            .into_iter()
            .find(|provider| provider.compatible)
        else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let video = temp.path().join("sample.mp4");
        let fixture = ProcessSpec {
            executable: provider.executable_path.clone(),
            args: vec![
                "-nostdin".into(),
                "-hide_banner".into(),
                "-loglevel".into(),
                "error".into(),
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "color=c=blue:s=96x64:r=8".into(),
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "sine=frequency=440:sample_rate=48000".into(),
                "-t".into(),
                "1.5".into(),
                "-c:v".into(),
                "libx264".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-c:a".into(),
                "aac".into(),
                video.as_os_str().to_os_string(),
            ],
            current_dir: Some(temp.path().to_path_buf()),
            timeout: Duration::from_secs(30),
            output_limit: 1024 * 1024,
        };
        let created = process::run(&fixture, &AtomicBool::new(false)).unwrap();
        assert!(
            created.status.success(),
            "fixture: {}",
            String::from_utf8_lossy(&created.stderr)
        );
        let runtime = crate::Arcade::in_memory().unwrap();
        let video_file = runtime.grants().grant(&video).unwrap();
        let run = |id: &str, selected: &SelectedFile, options: Value| {
            let manifest = runtime
                .list_tools()
                .into_iter()
                .find(|tool| tool.id == id)
                .unwrap();
            execute(
                &manifest,
                &ToolRequest {
                    tool_id: id.into(),
                    inputs: vec![selected.as_tool_value()],
                    options,
                },
                runtime.grants(),
                &AtomicBool::new(false),
            )
            .unwrap()
        };

        for (id, selected, options) in [
            (
                "arcade.video.trim",
                &video_file,
                json!({"startSeconds":0.2,"endSeconds":1.0}),
            ),
            (
                "arcade.video.crop",
                &video_file,
                json!({"width":48,"height":32,"rotation":"90"}),
            ),
            (
                "arcade.video.gif",
                &video_file,
                json!({"format":"gif","durationSeconds":0.5,"fps":6,"width":64}),
            ),
            (
                "arcade.video.frames",
                &video_file,
                json!({"mode":"timestamp","timestampSeconds":0.5}),
            ),
        ] {
            let result = run(id, selected, options);
            assert_eq!(
                result.status,
                ResultStatus::Success,
                "{id}: {:?}",
                result.message
            );
            assert!(!result.outputs.is_empty(), "{id} returned no output");
        }
    }
}
