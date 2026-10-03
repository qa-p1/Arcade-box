//! Video actions: one encoding plan shared by every tool that writes video,
//! stream-copy paths where they are valid, previews for the visual editors, and
//! batch execution for single-file transforms.
//!
//! Options arrive from standard forms, saved pipelines, and the CLI, so every
//! value is validated here. Provider calls use argument vectors only.

use super::job::{
    Job, MAX_BATCH, Progress, batch, clock, megabytes, output_name, produce, publish_named,
};
use super::{
    MediaInput, attach_provider, base_result, ffprobe_json, format_seconds, option_bool,
    option_choice_u64, option_f64, option_str, option_u64, optional_integer, provider_error,
    publish_staged, requested_output_name, run_ffmpeg, sanitize_piece, stage_directory,
    validate_range,
};
use crate::{
    grants::FileGrants,
    process::{self, ProcessSpec},
    provider::{ProviderInfo, discover_ffmpeg},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Tools that accept several files and process each one independently.
const BATCH_TOOLS: [&str; 4] = [
    "arcade.video.convert",
    "arcade.video.compress",
    "arcade.video.crop",
    "arcade.video.extract-audio",
];
const MAX_FRAMES: u64 = 5_000;
const BITMAP_SUBTITLES: [&str; 4] = ["hdmv_pgs_subtitle", "dvd_subtitle", "dvb_subtitle", "xsub"];

// ---------------------------------------------------------------------------
// Encoding plan
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Container {
    Mp4,
    Mkv,
    Webm,
    Mov,
}

impl Container {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "mp4" => Ok(Self::Mp4),
            "mkv" => Ok(Self::Mkv),
            "webm" => Ok(Self::Webm),
            "mov" => Ok(Self::Mov),
            other => Err(format!("Unsupported video output format: {other}")),
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Mov => "mov",
        }
    }

    fn muxer(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::Mkv => "matroska",
            Self::Webm => "webm",
            Self::Mov => "mov",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4",
            Self::Mkv => "Matroska",
            Self::Webm => "WebM",
            Self::Mov => "QuickTime",
        }
    }

    fn default_codec(self) -> VideoCodec {
        if self == Self::Webm {
            VideoCodec::Vp9
        } else {
            VideoCodec::H264
        }
    }

    /// Encoder and bitrate used when audio has to be (re-)encoded.
    fn audio_encoder(self) -> (&'static str, u64) {
        if self == Self::Webm {
            ("libopus", 160)
        } else {
            ("aac", 192)
        }
    }

    /// Whether an existing stream, named as ffprobe reports it, can be copied.
    fn holds_video(self, codec: &str) -> bool {
        match self {
            Self::Mkv => true,
            Self::Mp4 => matches!(codec, "h264" | "hevc" | "av1" | "mpeg4" | "vp9"),
            Self::Mov => matches!(codec, "h264" | "hevc" | "prores" | "mpeg4" | "mjpeg"),
            Self::Webm => matches!(codec, "vp8" | "vp9" | "av1"),
        }
    }

    fn holds_audio(self, codec: &str) -> bool {
        match self {
            Self::Mkv => true,
            Self::Mp4 => matches!(
                codec,
                "aac" | "mp3" | "ac3" | "eac3" | "opus" | "flac" | "alac"
            ),
            Self::Mov => {
                matches!(codec, "aac" | "mp3" | "ac3" | "alac") || codec.starts_with("pcm_")
            }
            Self::Webm => matches!(codec, "opus" | "vorbis"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum VideoCodec {
    H264,
    H265,
    Av1,
    Vp9,
    ProRes,
}

impl VideoCodec {
    fn encoder(self) -> &'static str {
        match self {
            Self::H264 => "libx264",
            Self::H265 => "libx265",
            Self::Av1 => "libsvtav1",
            Self::Vp9 => "libvpx-vp9",
            Self::ProRes => "prores_ks",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::H265 => "H.265",
            Self::Av1 => "AV1",
            Self::Vp9 => "VP9",
            Self::ProRes => "ProRes",
        }
    }

    fn fits(self, container: Container) -> bool {
        match container {
            Container::Mkv => true,
            Container::Mp4 => matches!(self, Self::H264 | Self::H265 | Self::Av1),
            Container::Mov => matches!(self, Self::H264 | Self::H265 | Self::ProRes),
            Container::Webm => matches!(self, Self::Vp9 | Self::Av1),
        }
    }

    fn max_crf(self) -> u64 {
        match self {
            Self::Av1 | Self::Vp9 => 63,
            _ => 51,
        }
    }

    /// Constant-quality value for a preset. ProRes uses a profile instead.
    fn crf(self, level: Quality) -> u64 {
        let table: [u64; 3] = match self {
            Self::H264 => [18, 23, 28],
            Self::H265 => [20, 26, 31],
            Self::Av1 => [24, 32, 40],
            Self::Vp9 => [24, 32, 40],
            Self::ProRes => [3, 2, 0],
        };
        table[level as usize]
    }

    fn supports_two_pass(self) -> bool {
        matches!(self, Self::H264 | Self::H265 | Self::Vp9)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Quality {
    High = 0,
    Balanced = 1,
    Small = 2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AudioMode {
    Encode,
    Copy,
    Remove,
}

#[derive(Clone, Copy, Debug)]
struct VideoPlan {
    container: Container,
    codec: VideoCodec,
    /// True when the user picked a codec instead of the container default.
    codec_chosen: bool,
    /// CRF, or the ProRes profile number.
    crf: u64,
    audio: AudioMode,
    audio_kbps: Option<u64>,
    max_height: Option<u64>,
}

fn plan(
    request: &ToolRequest,
    container_key: &str,
    default_container: &str,
    default_quality: Quality,
) -> Result<VideoPlan, String> {
    let container = Container::parse(option_str(request, container_key, default_container)?)?;
    let (codec, codec_chosen) = match option_str(request, "videoCodec", "auto")? {
        "auto" => (container.default_codec(), false),
        "h264" => (VideoCodec::H264, true),
        "h265" => (VideoCodec::H265, true),
        "av1" => (VideoCodec::Av1, true),
        "vp9" => (VideoCodec::Vp9, true),
        "prores" => (VideoCodec::ProRes, true),
        other => return Err(format!("Unsupported video codec: {other}")),
    };
    if !codec.fits(container) {
        return Err(format!(
            "{} video can't be saved as {}. Choose Matroska, or another codec.",
            codec.label(),
            container.label()
        ));
    }
    let level = match request.options.get("quality") {
        None | Some(Value::Null) => default_quality,
        Some(Value::String(value)) => match value.as_str() {
            "high" => Quality::High,
            "balanced" => Quality::Balanced,
            "small" => Quality::Small,
            other => return Err(format!("Unknown quality preset: {other}")),
        },
        // Older saved pipelines pass a CRF number as `quality`.
        Some(Value::Number(_)) => default_quality,
        Some(_) => return Err("quality must be a preset name".into()),
    };
    let crf = if codec == VideoCodec::ProRes {
        codec.crf(level)
    } else if request
        .options
        .get("crf")
        .is_some_and(|value| !value.is_null())
    {
        option_u64(request, "crf", 0, 0, codec.max_crf())?
    } else if let Some(Value::Number(_)) = request.options.get("quality") {
        option_u64(request, "quality", 0, 0, codec.max_crf())?
    } else {
        codec.crf(level)
    };
    let audio = match option_str(request, "audio", "auto")? {
        "auto" | "encode" => AudioMode::Encode,
        "copy" => AudioMode::Copy,
        "none" => AudioMode::Remove,
        other => return Err(format!("Unknown audio handling: {other}")),
    };
    let audio_kbps = match request.options.get("audioKbps") {
        None | Some(Value::Null) => None,
        Some(_) => Some(option_u64(request, "audioKbps", 192, 32, 512)?),
    };
    let max_height = match option_str(request, "maxHeight", "original")? {
        "original" => None,
        value => Some(
            value
                .parse::<u64>()
                .ok()
                .filter(|height| [2160, 1440, 1080, 720, 480, 360, 240].contains(height))
                .ok_or_else(|| format!("Unsupported maximum height: {value}"))?,
        ),
    };
    Ok(VideoPlan {
        container,
        codec,
        codec_chosen,
        crf,
        audio,
        audio_kbps,
        max_height,
    })
}

impl VideoPlan {
    fn capabilities(&self, required: &mut Vec<String>) {
        required.push(format!("encoder:{}", self.codec.encoder()));
        required.push(format!("mux:{}", self.container.muxer()));
        if self.audio != AudioMode::Remove {
            required.push(format!("encoder:{}", self.container.audio_encoder().0));
        }
    }

    fn video_args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec!["-c:v".into(), self.codec.encoder().into()];
        match self.codec {
            VideoCodec::ProRes => {
                args.extend([
                    "-profile:v".into(),
                    self.crf.to_string().into(),
                    "-pix_fmt".into(),
                    "yuv422p10le".into(),
                ]);
            }
            VideoCodec::Vp9 => args.extend([
                "-crf".into(),
                self.crf.to_string().into(),
                "-b:v".into(),
                "0".into(),
                "-deadline".into(),
                "good".into(),
                "-cpu-used".into(),
                "4".into(),
                "-row-mt".into(),
                "1".into(),
            ]),
            VideoCodec::Av1 => args.extend([
                "-crf".into(),
                self.crf.to_string().into(),
                "-preset".into(),
                "8".into(),
            ]),
            VideoCodec::H264 | VideoCodec::H265 => {
                args.extend([
                    "-crf".into(),
                    self.crf.to_string().into(),
                    "-preset".into(),
                    "medium".into(),
                ]);
                // 8-bit 4:2:0 keeps H.264 playable everywhere.
                if self.codec == VideoCodec::H264 {
                    args.extend(["-pix_fmt".into(), "yuv420p".into()]);
                }
            }
        }
        if self.codec == VideoCodec::H265
            && matches!(self.container, Container::Mp4 | Container::Mov)
        {
            args.extend(["-tag:v".into(), "hvc1".into()]);
        }
        args
    }

    /// Audio arguments for a source whose first audio codec is `source`.
    /// Copy falls back to encoding, with a note, when the container can't hold it.
    fn audio_args(&self, source: Option<&str>, warnings: &mut Vec<String>) -> Vec<OsString> {
        let Some(source) = source else {
            return vec!["-an".into()];
        };
        match self.audio {
            AudioMode::Remove => vec!["-an".into()],
            AudioMode::Copy if self.container.holds_audio(source) => {
                vec!["-c:a".into(), "copy".into()]
            }
            mode => {
                if mode == AudioMode::Copy {
                    warnings.push(format!(
                        "The original {source} audio can't be stored in {}, so it was re-encoded.",
                        self.container.label()
                    ));
                }
                let (encoder, default_kbps) = self.container.audio_encoder();
                vec![
                    "-c:a".into(),
                    encoder.into(),
                    "-b:a".into(),
                    format!("{}k", self.audio_kbps.unwrap_or(default_kbps)).into(),
                ]
            }
        }
    }

    fn container_args(&self) -> Vec<OsString> {
        let mut args = Vec::new();
        if matches!(self.container, Container::Mp4 | Container::Mov) {
            args.extend(["-movflags".into(), "+faststart".into()]);
        }
        args.extend(["-f".into(), self.container.muxer().into()]);
        args
    }

    fn scale_filter(&self) -> Option<String> {
        self.max_height
            .map(|height| format!("scale=-2:min({height}\\,ih)"))
    }

    fn describe(&self) -> String {
        format!("{} {}", self.container.label(), self.codec.label())
    }
}

// ---------------------------------------------------------------------------
// Source probing
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct VideoStream {
    codec: String,
    width: u64,
    height: u64,
    pix_fmt: String,
    frame_rate: Option<(u64, u64)>,
    rotation: i64,
}

impl VideoStream {
    /// Frame size as shown to the viewer, after rotation metadata.
    fn display_size(&self) -> (u64, u64) {
        if self.rotation.rem_euclid(180) == 90 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }

    fn fps(&self) -> Option<f64> {
        self.frame_rate
            .map(|(num, den)| num as f64 / den as f64)
            .filter(|fps| fps.is_finite() && *fps > 0.0)
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSummary {
    /// Position among streams of the same kind (the `:a:N` / `:s:N` index).
    pub index: usize,
    pub kind: String,
    pub codec: String,
    pub language: Option<String>,
    pub title: Option<String>,
    pub detail: String,
    /// Image-based subtitles (PGS, VobSub) can be burned but not converted to text.
    pub bitmap: bool,
}

#[derive(Clone, Debug, Default)]
struct SourceInfo {
    duration: Option<f64>,
    bytes: Option<u64>,
    video: Option<VideoStream>,
    audio: Vec<StreamSummary>,
    subtitles: Vec<StreamSummary>,
}

impl SourceInfo {
    fn first_audio(&self) -> Option<&str> {
        self.audio.first().map(|stream| stream.codec.as_str())
    }

    fn require_video(&self) -> Result<&VideoStream, String> {
        self.video
            .as_ref()
            .ok_or_else(|| "The selected file has no video stream".into())
    }
}

fn parse_rate(value: &Value) -> Option<(u64, u64)> {
    let (num, den) = value.as_str()?.split_once('/')?;
    let (num, den) = (num.parse().ok()?, den.parse().ok()?);
    (num > 0 && den > 0).then_some((num, den))
}

fn rotation_of(stream: &Value) -> i64 {
    let side = stream["side_data_list"].as_array().and_then(|list| {
        list.iter()
            .find_map(|entry| entry["rotation"].as_f64().map(|value| value.round() as i64))
    });
    side.or_else(|| {
        stream["tags"]["rotate"]
            .as_str()
            .and_then(|value| value.parse().ok())
    })
    .unwrap_or(0)
}

fn tag(stream: &Value, key: &str) -> Option<String> {
    stream["tags"][key]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "und")
        .map(|value| value.chars().take(80).collect())
}

fn probe(ffmpeg: &Path, input: &Path, cancelled: &AtomicBool) -> Result<SourceInfo, String> {
    let value = ffprobe_json(ffmpeg, input, cancelled)?;
    let mut info = SourceInfo {
        duration: value["format"]["duration"]
            .as_str()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value > 0.0),
        bytes: value["format"]["size"]
            .as_str()
            .and_then(|value| value.parse().ok()),
        ..SourceInfo::default()
    };
    for stream in value["streams"].as_array().into_iter().flatten() {
        let codec = stream["codec_name"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned();
        match stream["codec_type"].as_str() {
            Some("video") if info.video.is_none() && stream["disposition"]["attached_pic"] != 1 => {
                info.video = Some(VideoStream {
                    codec,
                    width: stream["width"].as_u64().unwrap_or(0),
                    height: stream["height"].as_u64().unwrap_or(0),
                    pix_fmt: stream["pix_fmt"].as_str().unwrap_or_default().to_owned(),
                    frame_rate: parse_rate(&stream["avg_frame_rate"])
                        .or_else(|| parse_rate(&stream["r_frame_rate"])),
                    rotation: rotation_of(stream),
                });
            }
            Some("audio") => {
                let channels = stream["channels"].as_u64().unwrap_or(0);
                let rate = stream["sample_rate"]
                    .as_str()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                let detail = [
                    (rate > 0).then(|| format!("{} kHz", rate as f64 / 1000.0)),
                    stream["channel_layout"]
                        .as_str()
                        .map(str::to_owned)
                        .or_else(|| (channels > 0).then(|| format!("{channels} ch"))),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
                info.audio.push(StreamSummary {
                    index: info.audio.len(),
                    kind: "audio".into(),
                    language: tag(stream, "language"),
                    title: tag(stream, "title"),
                    detail: format!(
                        "{codec}{}",
                        if detail.is_empty() {
                            String::new()
                        } else {
                            format!(" · {detail}")
                        }
                    ),
                    bitmap: false,
                    codec,
                });
            }
            Some("subtitle") => {
                let bitmap = BITMAP_SUBTITLES.contains(&codec.as_str());
                info.subtitles.push(StreamSummary {
                    index: info.subtitles.len(),
                    kind: "subtitle".into(),
                    language: tag(stream, "language"),
                    title: tag(stream, "title"),
                    detail: if bitmap {
                        format!("{codec} · image-based")
                    } else {
                        codec.clone()
                    },
                    bitmap,
                    codec,
                });
            }
            _ => {}
        }
    }
    Ok(info)
}

/// Keyframe timestamps in a window around `around`, in ascending order.
fn keyframes_near(
    ffmpeg: &Path,
    input: &Path,
    around: f64,
    cancelled: &AtomicBool,
) -> Result<Vec<f64>, String> {
    let from = (around - 30.0).max(0.0);
    let output = process::run(
        &ProcessSpec {
            executable: super::ffprobe_path(ffmpeg),
            args: vec![
                "-v".into(),
                "error".into(),
                "-select_streams".into(),
                "v:0".into(),
                "-skip_frame".into(),
                "nokey".into(),
                "-read_intervals".into(),
                format!("{}%{}", format_seconds(from), format_seconds(around + 1.0)).into(),
                "-show_entries".into(),
                "frame=best_effort_timestamp_time,pts_time".into(),
                "-of".into(),
                "csv=p=0".into(),
                input.as_os_str().to_os_string(),
            ],
            current_dir: None,
            timeout: Duration::from_secs(60),
            output_limit: 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("Could not read keyframe positions from the video".into());
    }
    let mut times: Vec<f64> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            line.split(',')
                .find_map(|piece| piece.trim().parse::<f64>().ok())
        })
        .filter(|time| time.is_finite())
        .collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    Ok(times)
}

/// `-fps_mode` replaced `-vsync` in FFmpeg 5.1; older builds only know `-vsync`.
fn variable_frame_rate_args(provider: &ProviderInfo) -> [OsString; 2] {
    let major = provider
        .version
        .trim_start_matches(['n', 'N'])
        .split(['.', '-'])
        .next()
        .and_then(|value| value.parse::<u32>().ok());
    let minor = provider
        .version
        .trim_start_matches(['n', 'N'])
        .split('.')
        .nth(1)
        .and_then(|value| value.split('-').next())
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    if matches!(major, Some(major) if major < 5 || (major == 5 && minor == 0)) {
        ["-vsync".into(), "vfr".into()]
    } else {
        ["-fps_mode".into(), "vfr".into()]
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Provider capabilities a video request needs before any work starts.
pub(super) fn required(
    id: &str,
    request: &ToolRequest,
    paths: &[PathBuf],
    required: &mut Vec<String>,
) -> Result<(), String> {
    match id {
        "arcade.video.convert" | "arcade.video.crop" | "arcade.video.join" => {
            plan(request, "format", "mp4", Quality::Balanced)?.capabilities(required);
            if id == "arcade.video.join" {
                required.extend(["filter:concat".into(), "filter:pad".into()]);
            }
            if id == "arcade.video.crop" {
                match option_str(request, "flip", "none")? {
                    "horizontal" => required.push("filter:hflip".into()),
                    "vertical" => required.push("filter:vflip".into()),
                    _ => {}
                }
            }
        }
        "arcade.video.trim" => {
            let plan = plan(request, "format", "mp4", Quality::High)?;
            if option_str(request, "mode", "auto")? == "fast" {
                required.push(format!("mux:{}", plan.container.muxer()));
            } else {
                plan.capabilities(required);
            }
        }
        "arcade.video.compress" => {
            plan(request, "format", "mp4", Quality::Small)?.capabilities(required);
        }
        "arcade.video.extract-audio" => match option_str(request, "format", "mp3")? {
            "copy" => {}
            format => {
                let format = super::audio_format(format)?;
                super::add_format_capabilities(required, format, false);
            }
        },
        "arcade.video.gif" => match option_str(request, "format", "gif")? {
            "gif" => required.extend([
                "encoder:gif".into(),
                "mux:gif".into(),
                "filter:palettegen".into(),
                "filter:paletteuse".into(),
            ]),
            "webp" => required.extend(["encoder:libwebp_anim".into(), "mux:webp".into()]),
            other => return Err(format!("Unsupported animation format: {other}")),
        },
        "arcade.video.frames" => {
            required.push("mux:image2".into());
            required.push(match option_str(request, "imageFormat", "png")? {
                "png" => "encoder:png".into(),
                "jpg" => "encoder:mjpeg".into(),
                other => return Err(format!("Unsupported frame image format: {other}")),
            });
            if option_str(request, "mode", "timestamp")? == "everyFrames" {
                required.push("filter:select".into());
            }
        }
        "arcade.video.subtitles" => match option_str(request, "operation", "extract")? {
            "extract" | "convert" => {
                let format = option_str(request, "outputFormat", "srt")?;
                required.push(format!("encoder:{}", subtitle_codec(format)?));
                required.push(format!("mux:{}", subtitle_muxer(format)?));
            }
            "remove" => {
                let container = Container::parse(option_str(request, "videoFormat", "mkv")?)?;
                required.push(format!("mux:{}", container.muxer()));
            }
            "add" => {
                let container = Container::parse(option_str(request, "videoFormat", "mkv")?)?;
                required.push(format!("mux:{}", container.muxer()));
                match container {
                    Container::Mp4 | Container::Mov => required.push("encoder:mov_text".into()),
                    Container::Webm => required.push("encoder:webvtt".into()),
                    Container::Mkv => {}
                }
            }
            "burn" => {
                plan(request, "videoFormat", "mkv", Quality::High)?.capabilities(required);
                if paths.len() == 2 {
                    required.push("filter:subtitles".into());
                }
            }
            other => return Err(format!("Unknown subtitle operation: {other}")),
        },
        _ => return Err(format!("No video executor is registered for {id}")),
    }
    Ok(())
}

pub(super) fn validate_count(id: &str, request: &ToolRequest, count: usize) -> Result<(), String> {
    if BATCH_TOOLS.contains(&id) {
        return if (1..=MAX_BATCH).contains(&count) {
            Ok(())
        } else {
            Err(format!("Select between 1 and {MAX_BATCH} videos"))
        };
    }
    match id {
        "arcade.video.join" if (2..=super::MAX_INPUTS).contains(&count) => Ok(()),
        "arcade.video.join" => Err("Select at least two clips to join".into()),
        "arcade.video.subtitles" => match option_str(request, "operation", "extract")? {
            "add" if count == 2 => Ok(()),
            "add" => Err("Adding a track needs the video and one subtitle file".into()),
            "burn" if (1..=2).contains(&count) => Ok(()),
            "burn" => Err("Burn needs the video and, optionally, one subtitle file".into()),
            "convert" | "extract" | "remove" if count == 1 => Ok(()),
            "convert" => Err("Subtitle conversion takes one subtitle file".into()),
            "extract" | "remove" => Err("Choose one video".into()),
            other => Err(format!("Unknown subtitle operation: {other}")),
        },
        _ if count == 1 => Ok(()),
        _ => Err("Choose one video".into()),
    }
}

pub(super) fn execute(
    manifest: &ToolManifest,
    provider: &ProviderInfo,
    paths: &[PathBuf],
    request: &ToolRequest,
    grants: &FileGrants,
    cancelled: &AtomicBool,
    progress: Progress,
) -> Result<ToolResult, String> {
    let job = Job {
        manifest,
        provider,
        request,
        grants,
        cancelled,
        progress,
    };
    if BATCH_TOOLS.contains(&manifest.id.as_str()) && paths.len() > 1 {
        return batch(&job, paths, single);
    }
    single(&job, paths)
}

fn single(job: &Job, paths: &[PathBuf]) -> Result<ToolResult, String> {
    let input = paths.first().ok_or("Choose a video first")?;
    match job.manifest.id.as_str() {
        "arcade.video.convert" => convert(job, input),
        "arcade.video.trim" => trim(job, input),
        "arcade.video.crop" => crop(job, input),
        "arcade.video.compress" => compress(job, input),
        "arcade.video.join" => join(job, paths),
        "arcade.video.extract-audio" => extract_audio(job, input),
        "arcade.video.gif" => animation(job, input),
        "arcade.video.frames" => frames(job, input),
        "arcade.video.subtitles" => subtitles(job, paths),
        other => Err(format!("No video executor is registered for {other}")),
    }
}

fn even(value: u64) -> u64 {
    value - value % 2
}

fn audio_maps(plan: &VideoPlan, info: &SourceInfo, args: &mut Vec<OsString>) {
    if plan.audio != AudioMode::Remove && !info.audio.is_empty() {
        args.extend(["-map".into(), "0:a?".into()]);
    }
}

// ---------------------------------------------------------------------------
// Convert
// ---------------------------------------------------------------------------

fn convert(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let plan = plan(job.request, "format", "mp4", Quality::Balanced)?;
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    info.require_video()?;
    let mut warnings = Vec::new();
    let mut args: Vec<OsString> = vec!["-map".into(), "0:v:0".into()];
    audio_maps(&plan, &info, &mut args);
    if let Some(filter) = plan.scale_filter() {
        args.extend(["-vf".into(), filter.into()]);
    }
    args.extend(plan.video_args());
    args.extend(plan.audio_args(info.first_audio(), &mut warnings));
    args.extend(plan.container_args());
    let stage = stage_directory(input)?;
    let mut result = produce(
        job,
        input,
        stage.path(),
        &[MediaInput::plain(input)],
        args,
        plan.container.extension(),
        "converted",
        info.duration,
        None,
    )?;
    result.message = Some(format!(
        "Saved {} ({})",
        output_name(&result),
        plan.describe()
    ));
    result.warnings = warnings;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Trim: stream copy when the cut allows it, re-encode when it doesn't
// ---------------------------------------------------------------------------

fn trim(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let plan = plan(job.request, "format", "mp4", Quality::High)?;
    let (start, end) = validate_range(job.request)?;
    if start.is_none() && end.is_none() {
        return Err("Choose a start or end time to trim".into());
    }
    let ffmpeg = &job.provider.executable_path;
    let info = probe(ffmpeg, input, job.cancelled)?;
    let video = info.require_video()?.clone();
    let start = start.unwrap_or(0.0);
    if let Some(total) = info.duration
        && start >= total
    {
        return Err(format!(
            "The start time is after the end of the video ({}).",
            clock(total)
        ));
    }
    let end = end.map(|end| info.duration.map_or(end, |total| end.min(total)));
    let tolerance = video.fps().map_or(0.02, |fps| 0.5 / fps).max(0.001);
    let mode = option_str(job.request, "mode", "auto")?;
    let can_copy = plan.container.holds_video(&video.codec);
    let (copy, from, note) = match mode {
        "precise" => (false, start, None),
        "fast" => {
            if !can_copy {
                return Err(format!(
                    "Fast trim keeps the original {} video, which {} can't hold. Choose Matroska, or use Precise.",
                    video.codec,
                    plan.container.label()
                ));
            }
            let keyframe = if start < 0.001 {
                0.0
            } else {
                keyframes_near(ffmpeg, input, start, job.cancelled)?
                    .into_iter()
                    .rfind(|time| *time <= start + tolerance)
                    .unwrap_or(0.0)
            };
            let note = ((start - keyframe).abs() > tolerance).then(|| {
                format!(
                    "Started at the nearest earlier keyframe ({}) so the video didn't need re-encoding.",
                    clock(keyframe)
                )
            });
            (true, keyframe, note)
        }
        "auto" => {
            let eligible = can_copy && !plan.codec_chosen && plan.max_height.is_none();
            if !eligible {
                (false, start, None)
            } else if start < 0.001 {
                (true, 0.0, None)
            } else {
                let aligned = keyframes_near(ffmpeg, input, start, job.cancelled)
                    .unwrap_or_default()
                    .into_iter()
                    .find(|time| (time - start).abs() <= tolerance);
                match aligned {
                    Some(keyframe) => (true, keyframe, None),
                    None => (
                        false,
                        start,
                        Some("Re-encoded for a frame-accurate start, because the start time isn't on a keyframe.".to_string()),
                    ),
                }
            }
        }
        other => return Err(format!("Unknown trim mode: {other}")),
    };
    let length = end.map(|end| end - from).filter(|length| *length > 0.0);
    let input_options = if from > 0.0 {
        vec!["-ss".into(), format_seconds(from).into()]
    } else {
        Vec::new()
    };
    let mut args: Vec<OsString> = Vec::new();
    if let Some(length) = length {
        args.extend(["-t".into(), format_seconds(length).into()]);
    }
    args.extend(["-map".into(), "0:v:0".into()]);
    audio_maps(&plan, &info, &mut args);
    let mut warnings = Vec::new();
    if copy {
        args.extend(["-c:v".into(), "copy".into()]);
        match info.first_audio() {
            None => {}
            Some(_) if plan.audio == AudioMode::Remove => args.push("-an".into()),
            Some(codec) if plan.container.holds_audio(codec) => {
                args.extend(["-c:a".into(), "copy".into()]);
            }
            Some(_) => {
                // The container can't hold the original audio; encode just that.
                let copy_plan = VideoPlan {
                    audio: AudioMode::Copy,
                    ..plan
                };
                args.extend(copy_plan.audio_args(info.first_audio(), &mut warnings));
            }
        }
        args.extend(["-avoid_negative_ts".into(), "make_zero".into()]);
    } else {
        if let Some(filter) = plan.scale_filter() {
            args.extend(["-vf".into(), filter.into()]);
        }
        args.extend(plan.video_args());
        args.extend(plan.audio_args(info.first_audio(), &mut warnings));
    }
    args.extend(plan.container_args());
    let stage = stage_directory(input)?;
    let duration = length.or_else(|| info.duration.map(|total| total - from));
    let mut result = produce(
        job,
        input,
        stage.path(),
        &[MediaInput {
            options: input_options,
            path: input.to_path_buf(),
        }],
        args,
        plan.container.extension(),
        "trimmed",
        duration,
        None,
    )?;
    result.message = Some(if copy {
        format!("Saved {} without re-encoding", output_name(&result))
    } else {
        format!("Saved {}", output_name(&result))
    });
    result.metadata.insert(
        "method".into(),
        json!(if copy { "copy" } else { "reencode" }),
    );
    result.warnings = note.into_iter().chain(warnings).collect();
    Ok(result)
}

// ---------------------------------------------------------------------------
// Crop / resize / rotate / flip
// ---------------------------------------------------------------------------

fn parse_aspect(value: &str) -> Result<Option<(u64, u64)>, String> {
    match value {
        "free" => Ok(None),
        "16:9" => Ok(Some((16, 9))),
        "9:16" => Ok(Some((9, 16))),
        "1:1" => Ok(Some((1, 1))),
        "4:3" => Ok(Some((4, 3))),
        "3:4" => Ok(Some((3, 4))),
        "4:5" => Ok(Some((4, 5))),
        "21:9" => Ok(Some((21, 9))),
        other => Err(format!("Unsupported aspect ratio: {other}")),
    }
}

/// The largest centered rectangle with the requested aspect ratio.
fn centered_crop(width: u64, height: u64, aspect: (u64, u64)) -> (u64, u64, u64, u64) {
    let (aw, ah) = aspect;
    let (crop_w, crop_h) = if width * ah > height * aw {
        (height * aw / ah, height)
    } else {
        (width, width * ah / aw)
    };
    let (crop_w, crop_h) = (even(crop_w).max(2), even(crop_h).max(2));
    (
        crop_w,
        crop_h,
        (width.saturating_sub(crop_w)) / 2,
        (height.saturating_sub(crop_h)) / 2,
    )
}

fn crop(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let request = job.request;
    let plan = plan(request, "format", "mp4", Quality::High)?;
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    let (source_w, source_h) = info.require_video()?.display_size();
    let width = optional_integer(request, "width", 2, 7680)?;
    let height = optional_integer(request, "height", 2, 4320)?;
    let mut rect = match (
        optional_integer(request, "cropWidth", 2, 7680)?,
        optional_integer(request, "cropHeight", 2, 4320)?,
    ) {
        (Some(w), Some(h)) => Some((
            w,
            h,
            optional_integer(request, "cropX", 0, 7680)?.unwrap_or(0),
            optional_integer(request, "cropY", 0, 4320)?.unwrap_or(0),
        )),
        (None, None) => None,
        _ => return Err("Set both the crop width and height".into()),
    };
    if rect.is_none()
        && let Some(aspect) = parse_aspect(option_str(request, "aspect", "free")?)?
    {
        if source_w == 0 || source_h == 0 {
            return Err("Could not read the video size to apply the aspect ratio".into());
        }
        rect = Some(centered_crop(source_w, source_h, aspect));
    }
    let rotation = option_choice_u64(request, "rotation", 0, &[0, 90, 180, 270])?;
    let flip = option_str(request, "flip", "none")?;
    let mut filters = Vec::new();
    if let Some((w, h, x, y)) = rect {
        if source_w > 0 && (x + w > source_w || y + h > source_h) {
            return Err(format!(
                "The crop area ({w}×{h} at {x},{y}) goes past the {source_w}×{source_h} frame."
            ));
        }
        filters.push(format!(
            "crop={}:{}:{x}:{y}",
            even(w).max(2),
            even(h).max(2)
        ));
    }
    match (width, height) {
        (None, None) => {}
        (Some(w), Some(h)) => filters.push(format!(
            "scale={w}:{h}:force_original_aspect_ratio=decrease:force_divisible_by=2"
        )),
        (Some(w), None) => filters.push(format!("scale={w}:-2")),
        (None, Some(h)) => filters.push(format!("scale=-2:{h}")),
    }
    match rotation {
        90 => filters.push("transpose=clock".into()),
        180 => filters.push("hflip,vflip".into()),
        270 => filters.push("transpose=cclock".into()),
        _ => {}
    }
    match flip {
        "none" => {}
        "horizontal" => filters.push("hflip".into()),
        "vertical" => filters.push("vflip".into()),
        other => return Err(format!("Unknown flip direction: {other}")),
    }
    if filters.is_empty() {
        return Err("Choose a crop, aspect ratio, size, rotation, or flip".into());
    }
    let mut warnings = Vec::new();
    let mut args: Vec<OsString> = vec!["-map".into(), "0:v:0".into()];
    audio_maps(&plan, &info, &mut args);
    args.extend(["-vf".into(), filters.join(",").into()]);
    args.extend(plan.video_args());
    args.extend(plan.audio_args(info.first_audio(), &mut warnings));
    args.extend(plan.container_args());
    let stage = stage_directory(input)?;
    let mut result = produce(
        job,
        input,
        stage.path(),
        &[MediaInput::plain(input)],
        args,
        plan.container.extension(),
        "edited",
        info.duration,
        None,
    )?;
    result.warnings = warnings;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Compress
// ---------------------------------------------------------------------------

enum Rate {
    Quality,
    Bitrate { video_kbps: u64, audio_kbps: u64 },
}

struct CompressSettings {
    filter: Option<String>,
    rate: Rate,
    warnings: Vec<String>,
}

fn compress_settings(
    request: &ToolRequest,
    plan: &VideoPlan,
    info: &SourceInfo,
) -> Result<CompressSettings, String> {
    let mut warnings = Vec::new();
    match option_str(request, "mode", "quality")? {
        "quality" => Ok(CompressSettings {
            filter: plan.scale_filter(),
            rate: Rate::Quality,
            warnings,
        }),
        "resolution" => {
            let height = option_choice_u64(
                request,
                "resolution",
                1080,
                &[2160, 1440, 1080, 720, 480, 360, 240],
            )?;
            if let Some(video) = &info.video
                && video.display_size().1 <= height
            {
                warnings.push(format!(
                    "The video is already {}p or smaller, so only the quality setting reduced its size.",
                    video.display_size().1
                ));
            }
            Ok(CompressSettings {
                filter: Some(format!("scale=-2:min({height}\\,ih)")),
                rate: Rate::Quality,
                warnings,
            })
        }
        "targetSize" => {
            let megabytes = option_f64(request, "targetSizeMb", 100.0, 0.1, 100_000.0)?;
            let audio_kbps = option_u64(request, "audioKbps", 128, 32, 512)?;
            target_size(
                plan,
                info,
                megabytes,
                audio_kbps,
                plan.scale_filter(),
                None,
                warnings,
            )
        }
        "preset" => {
            // Sizes leave headroom under each service's limit.
            let (megabytes, label) = match option_str(request, "preset", "discord")? {
                "discord" => (9.5, "Discord (10 MB)"),
                "discord-nitro" => (48.0, "Discord Nitro Basic (50 MB)"),
                "whatsapp" => (15.0, "WhatsApp (16 MB)"),
                "email" => (18.0, "email (25 MB attachment limit)"),
                other => return Err(format!("Unknown preset: {other}")),
            };
            let mut settings = target_size(
                plan,
                info,
                megabytes,
                96,
                Some("scale=-2:min(720\\,ih)".into()),
                Some(2500.0),
                warnings,
            )?;
            settings
                .warnings
                .retain(|warning| !warning.contains("may not be smaller"));
            if let Some(bytes) = info.bytes
                && (bytes as f64) <= megabytes * 1024.0 * 1024.0
            {
                settings
                    .warnings
                    .push(format!("The original is already small enough for {label}."));
            }
            Ok(settings)
        }
        other => Err(format!("Unknown compression mode: {other}")),
    }
}

/// Bitrates that land a video near `megabytes`. `max_video_kbps` stops short
/// clips from being inflated far beyond what their resolution needs.
fn target_size(
    plan: &VideoPlan,
    info: &SourceInfo,
    megabytes: f64,
    audio_kbps: u64,
    filter: Option<String>,
    max_video_kbps: Option<f64>,
    mut warnings: Vec<String>,
) -> Result<CompressSettings, String> {
    if plan.codec == VideoCodec::ProRes {
        return Err("Target size isn't available for ProRes; choose another codec".into());
    }
    let seconds = info
        .duration
        .ok_or("Could not read the video duration needed for a target size")?;
    let audio_kbps = if plan.audio == AudioMode::Remove || info.audio.is_empty() {
        0
    } else {
        audio_kbps
    };
    let total_kbps = megabytes * 1024.0 * 1024.0 * 8.0 / seconds / 1000.0;
    // Leave about 2% for container overhead.
    let mut video_kbps = (total_kbps * 0.98 - audio_kbps as f64).floor();
    if video_kbps < 64.0 {
        let minimum =
            ((64.0 + audio_kbps as f64) * seconds * 1000.0 / 8.0 / 0.98) / (1024.0 * 1024.0);
        return Err(format!(
            "{megabytes} MB is too small for a {} video. Try at least {:.1} MB, or lower the audio bitrate.",
            clock(seconds),
            minimum.ceil()
        ));
    }
    if let Some(cap) = max_video_kbps {
        video_kbps = video_kbps.min(cap);
    }
    if let Some(bytes) = info.bytes
        && megabytes * 1024.0 * 1024.0 >= bytes as f64
    {
        warnings.push(format!(
            "The target is larger than the original ({}); the result may not be smaller.",
            megabytes_label(bytes)
        ));
    }
    Ok(CompressSettings {
        filter,
        rate: Rate::Bitrate {
            video_kbps: video_kbps as u64,
            audio_kbps,
        },
        warnings,
    })
}

fn megabytes_label(bytes: u64) -> String {
    megabytes(bytes)
}

fn bitrate_video_args(plan: &VideoPlan, kbps: u64) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "-c:v".into(),
        plan.codec.encoder().into(),
        "-b:v".into(),
        format!("{kbps}k").into(),
    ];
    match plan.codec {
        VideoCodec::H264 => args.extend([
            "-preset".into(),
            "medium".into(),
            "-pix_fmt".into(),
            "yuv420p".into(),
        ]),
        VideoCodec::H265 => args.extend(["-preset".into(), "medium".into()]),
        VideoCodec::Vp9 => args.extend([
            "-deadline".into(),
            "good".into(),
            "-cpu-used".into(),
            "4".into(),
            "-row-mt".into(),
            "1".into(),
        ]),
        VideoCodec::Av1 => args.extend(["-preset".into(), "8".into()]),
        VideoCodec::ProRes => {}
    }
    if plan.codec == VideoCodec::H265 && matches!(plan.container, Container::Mp4 | Container::Mov) {
        args.extend(["-tag:v".into(), "hvc1".into()]);
    }
    args
}

fn pass_args(codec: VideoCodec, pass: u8) -> Vec<OsString> {
    if codec == VideoCodec::H265 {
        vec![
            "-x265-params".into(),
            format!("pass={pass}:stats=x265.stats").into(),
        ]
    } else {
        vec![
            "-pass".into(),
            pass.to_string().into(),
            "-passlogfile".into(),
            "ffpass".into(),
        ]
    }
}

/// Encoder arguments for the compressed video and audio, shared by the real
/// run and by the sample encodes behind the size estimate.
fn compress_stream_args(
    plan: &VideoPlan,
    settings: &CompressSettings,
    info: &SourceInfo,
    warnings: &mut Vec<String>,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();
    if let Some(filter) = &settings.filter {
        args.extend(["-vf".into(), filter.into()]);
    }
    match settings.rate {
        Rate::Quality => {
            args.extend(plan.video_args());
            args.extend(plan.audio_args(info.first_audio(), warnings));
        }
        Rate::Bitrate {
            video_kbps,
            audio_kbps,
        } => {
            args.extend(bitrate_video_args(plan, video_kbps));
            if audio_kbps == 0 || info.audio.is_empty() {
                args.push("-an".into());
            } else {
                let encoding = VideoPlan {
                    audio: AudioMode::Encode,
                    audio_kbps: Some(audio_kbps),
                    ..*plan
                };
                args.extend(encoding.audio_args(info.first_audio(), warnings));
            }
        }
    }
    args
}

fn compress(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let plan = plan(job.request, "format", "mp4", Quality::Small)?;
    let ffmpeg = &job.provider.executable_path;
    let info = probe(ffmpeg, input, job.cancelled)?;
    info.require_video()?;
    let settings = compress_settings(job.request, &plan, &info)?;
    let mut warnings = settings.warnings.clone();
    let stage = stage_directory(input)?;
    let two_pass = matches!(settings.rate, Rate::Bitrate { .. }) && plan.codec.supports_two_pass();
    let mut maps: Vec<OsString> = vec!["-map".into(), "0:v:0".into()];
    audio_maps(&plan, &info, &mut maps);
    let mut args = maps.clone();
    args.extend(compress_stream_args(&plan, &settings, &info, &mut warnings));
    let item = if two_pass {
        // The first pass analyses the video only and writes nothing but stats.
        let mut first: Vec<OsString> = vec!["-map".into(), "0:v:0".into()];
        if let Some(filter) = &settings.filter {
            first.extend(["-vf".into(), filter.into()]);
        }
        if let Rate::Bitrate { video_kbps, .. } = settings.rate {
            first.extend(bitrate_video_args(&plan, video_kbps));
        }
        first.extend(pass_args(plan.codec, 1));
        first.extend(["-an".into(), "-f".into(), "null".into(), "-".into()]);
        let progress = job.progress.clone();
        run_ffmpeg(
            ffmpeg,
            &[MediaInput::plain(input)],
            first,
            stage.path(),
            info.duration,
            job.cancelled,
            Arc::new(move |fraction| progress(fraction * 0.5)),
            "error",
        )?;
        args.extend(pass_args(plan.codec, 2));
        let progress = job.progress.clone();
        Some(job.with_progress(Arc::new(move |fraction| progress(0.5 + fraction * 0.5))))
    } else {
        None
    };
    args.extend(plan.container_args());
    let runner = item.as_ref().unwrap_or(job);
    let mut result = produce(
        runner,
        input,
        stage.path(),
        &[MediaInput::plain(input)],
        args,
        plan.container.extension(),
        "compressed",
        info.duration,
        None,
    )?;
    let output_bytes = result.metadata.get("outputBytes").and_then(Value::as_u64);
    if let (Some(before), Some(after)) = (info.bytes, output_bytes) {
        result.metadata.insert("sourceBytes".into(), json!(before));
        if after < before && before > 0 {
            let saved = 100.0 - after as f64 * 100.0 / before as f64;
            result.message = Some(format!(
                "Saved {} — {saved:.0}% smaller ({} → {})",
                output_name(&result),
                megabytes(before),
                megabytes(after)
            ));
        } else {
            warnings.push(format!(
                "The result ({}) isn't smaller than the original ({}). Try a smaller quality preset or a lower resolution.",
                megabytes(after),
                megabytes(before)
            ));
        }
    }
    if matches!(settings.rate, Rate::Bitrate { .. }) {
        result.metadata.insert("twoPass".into(), json!(two_pass));
    }
    result.warnings = warnings;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Join: remux when clips match, otherwise a controlled transcode
// ---------------------------------------------------------------------------

/// Why the clips can't be joined without re-encoding, or `None` if they can.
fn copy_mismatch(plan: &VideoPlan, infos: &[SourceInfo]) -> Option<String> {
    let first = infos.first()?;
    let video = first.video.as_ref()?;
    if !plan.container.holds_video(&video.codec) {
        return Some(format!(
            "{} can't hold the clips' {} video",
            plan.container.label(),
            video.codec
        ));
    }
    if let Some(audio) = first.first_audio()
        && !plan.container.holds_audio(audio)
    {
        return Some(format!(
            "{} can't hold the clips' {audio} audio",
            plan.container.label()
        ));
    }
    for (index, info) in infos.iter().enumerate().skip(1) {
        let clip = index + 1;
        let Some(other) = &info.video else {
            return Some(format!("clip {clip} has no video"));
        };
        if other.codec != video.codec {
            return Some(format!(
                "clip {clip} uses {} video and clip 1 uses {}",
                other.codec, video.codec
            ));
        }
        if (other.width, other.height) != (video.width, video.height) {
            return Some(format!(
                "clip {clip} is {}×{} and clip 1 is {}×{}",
                other.width, other.height, video.width, video.height
            ));
        }
        if other.pix_fmt != video.pix_fmt || other.rotation != video.rotation {
            return Some(format!("clip {clip} uses a different picture format"));
        }
        match (other.fps(), video.fps()) {
            (Some(a), Some(b)) if (a - b).abs() > 0.01 => {
                return Some(format!(
                    "clip {clip} runs at {a:.2} fps and clip 1 at {b:.2} fps"
                ));
            }
            _ => {}
        }
        match (info.audio.first(), first.audio.first()) {
            (None, None) => {}
            (Some(a), Some(b)) if a.codec == b.codec && a.detail == b.detail => {}
            (Some(_), Some(_)) => return Some(format!("clip {clip} has different audio")),
            _ => return Some("only some clips have audio".into()),
        }
    }
    None
}

fn join(job: &Job, inputs: &[PathBuf]) -> Result<ToolResult, String> {
    let plan = plan(job.request, "format", "mp4", Quality::High)?;
    let ffmpeg = &job.provider.executable_path;
    let mut infos = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        let info = probe(ffmpeg, input, job.cancelled)?;
        if info.video.is_none() {
            return Err(format!(
                "Clip {} ({}) has no video stream",
                index + 1,
                input.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        infos.push(info);
    }
    let total: Option<f64> = infos.iter().map(|info| info.duration).sum();
    let mode = option_str(job.request, "mode", "auto")?;
    if !matches!(mode, "auto" | "reencode") {
        return Err(format!("Unknown join mode: {mode}"));
    }
    let custom = plan.codec_chosen || plan.max_height.is_some() || plan.audio == AudioMode::Remove;
    let mismatch = copy_mismatch(&plan, &infos);
    let mut note = None;
    if mode == "auto" && !custom {
        match &mismatch {
            None => match join_copy(job, &plan, inputs, total) {
                Ok(result) => return Ok(result),
                Err(error) if job.cancelled.load(Ordering::Relaxed) => return Err(error),
                Err(_) => {
                    note = Some(
                        "The clips couldn't be joined losslessly, so they were re-encoded."
                            .to_string(),
                    );
                }
            },
            Some(reason) => {
                note = Some(format!(
                    "Re-encoded to match the first clip because {reason}."
                ));
            }
        }
    }
    let mut result = join_transcode(job, &plan, inputs, &infos, total)?;
    result.warnings.splice(0..0, note);
    Ok(result)
}

fn join_copy(
    job: &Job,
    plan: &VideoPlan,
    inputs: &[PathBuf],
    total: Option<f64>,
) -> Result<ToolResult, String> {
    let stage = stage_directory(&inputs[0])?;
    let list = stage.path().join("clips.ffconcat");
    let mut script = String::from("ffconcat version 1.0\n");
    for input in inputs {
        let path = input
            .to_str()
            .ok_or("A clip path isn't valid Unicode, so it can't be joined losslessly")?;
        if path.contains(['\n', '\r']) {
            return Err("A clip path contains a line break".into());
        }
        script.push_str(&format!("file '{}'\n", path.replace('\'', "'\\''")));
    }
    fs::write(&list, script).map_err(|error| format!("Cannot prepare the clip list: {error}"))?;
    let mut args: Vec<OsString> = vec![
        "-map".into(),
        "0:v".into(),
        "-map".into(),
        "0:a?".into(),
        "-c".into(),
        "copy".into(),
    ];
    args.extend(plan.container_args());
    let mut result = produce(
        job,
        &inputs[0],
        stage.path(),
        &[MediaInput {
            options: vec!["-f".into(), "concat".into(), "-safe".into(), "0".into()],
            path: list,
        }],
        args,
        plan.container.extension(),
        "joined",
        total,
        None,
    )?;
    result.message = Some(format!(
        "Joined {} clips into {} without re-encoding",
        inputs.len(),
        output_name(&result)
    ));
    result.metadata.insert("method".into(), json!("copy"));
    Ok(result)
}

fn join_transcode(
    job: &Job,
    plan: &VideoPlan,
    inputs: &[PathBuf],
    infos: &[SourceInfo],
    total: Option<f64>,
) -> Result<ToolResult, String> {
    let first = infos[0].require_video()?;
    let (mut width, mut height) = first.display_size();
    if width == 0 || height == 0 {
        (width, height) = (1280, 720);
    }
    let (width, height) = (even(width).max(2), even(height).max(2));
    let fps = first
        .frame_rate
        .filter(|(num, den)| *num as f64 / *den as f64 <= 240.0)
        .map(|(num, den)| format!("{num}/{den}"))
        .unwrap_or_else(|| "30".into());
    let with_audio =
        plan.audio != AudioMode::Remove && infos.iter().any(|info| !info.audio.is_empty());
    let mut graph = String::new();
    for (index, info) in infos.iter().enumerate() {
        let scale = plan
            .max_height
            .map(|limit| format!(",scale=-2:min({limit}\\,ih)"))
            .unwrap_or_default();
        graph.push_str(&format!(
            "[{index}:v:0]scale={width}:{height}:force_original_aspect_ratio=decrease,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps}{scale}[v{index}];"
        ));
        if with_audio {
            if info.audio.is_empty() {
                let seconds = info.duration.ok_or_else(|| {
                    format!("Clip {} has no audio and an unknown length", index + 1)
                })?;
                graph.push_str(&format!(
                    "anullsrc=r=48000:cl=stereo,atrim=duration={}[a{index}];",
                    format_seconds(seconds)
                ));
            } else {
                graph.push_str(&format!(
                    "[{index}:a:0]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[a{index}];"
                ));
            }
        }
    }
    for index in 0..infos.len() {
        graph.push_str(&format!("[v{index}]"));
        if with_audio {
            graph.push_str(&format!("[a{index}]"));
        }
    }
    graph.push_str(&format!(
        "concat=n={}:v=1:a={}[outv]{}",
        infos.len(),
        u8::from(with_audio),
        if with_audio { "[outa]" } else { "" }
    ));
    let mut args: Vec<OsString> = vec![
        "-filter_complex".into(),
        graph.into(),
        "-map".into(),
        "[outv]".into(),
    ];
    if with_audio {
        args.extend(["-map".into(), "[outa]".into()]);
    }
    args.extend(plan.video_args());
    let mut warnings = Vec::new();
    let encode = VideoPlan {
        audio: AudioMode::Encode,
        ..*plan
    };
    args.extend(encode.audio_args(with_audio.then_some("pcm_f32le"), &mut warnings));
    args.extend(plan.container_args());
    let stage = stage_directory(&inputs[0])?;
    let sources: Vec<MediaInput> = inputs.iter().map(|path| MediaInput::plain(path)).collect();
    let mut result = produce(
        job,
        &inputs[0],
        stage.path(),
        &sources,
        args,
        plan.container.extension(),
        "joined",
        total,
        None,
    )?;
    result.message = Some(format!(
        "Joined {} clips into {} ({width}×{height})",
        inputs.len(),
        output_name(&result)
    ));
    result.metadata.insert("method".into(), json!("reencode"));
    result.warnings = warnings;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Extract audio
// ---------------------------------------------------------------------------

/// Standalone file type for an audio stream copied without re-encoding.
pub(super) fn copy_target(codec: &str) -> Option<(&'static str, &'static str)> {
    match codec {
        "aac" | "alac" => Some(("m4a", "ipod")),
        "mp3" => Some(("mp3", "mp3")),
        "opus" => Some(("opus", "ogg")),
        "vorbis" => Some(("ogg", "ogg")),
        "flac" => Some(("flac", "flac")),
        codec if codec.starts_with("pcm_") => Some(("wav", "wav")),
        _ => None,
    }
}

fn extract_audio(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    if info.audio.is_empty() {
        return Err("This video has no audio track".into());
    }
    let track = option_u64(job.request, "track", 0, 0, 63)? as usize;
    let stream = info.audio.get(track).ok_or_else(|| {
        format!(
            "Audio track {track} doesn't exist; this file has {} (numbered from 0)",
            info.audio.len()
        )
    })?;
    let has = |capability: &str| {
        job.provider
            .capabilities
            .iter()
            .any(|have| have == capability)
    };
    let mut warnings = Vec::new();
    let mut args: Vec<OsString> = vec![
        "-map".into(),
        format!("0:a:{track}").into(),
        "-vn".into(),
        "-sn".into(),
        "-dn".into(),
    ];
    let format = option_str(job.request, "format", "mp3")?;
    let extension = if format == "copy" {
        match copy_target(&stream.codec) {
            Some((extension, muxer)) if has(&format!("mux:{muxer}")) => {
                args.extend(["-c:a".into(), "copy".into(), "-f".into(), muxer.into()]);
                extension
            }
            _ => {
                if !has("encoder:flac") || !has("mux:flac") {
                    return Err(provider_error(&["encoder:flac".into(), "mux:flac".into()]));
                }
                warnings.push(format!(
                    "{} audio can't be saved on its own without conversion, so it was saved as lossless FLAC.",
                    stream.codec
                ));
                args.extend(["-c:a".into(), "flac".into(), "-f".into(), "flac".into()]);
                "flac"
            }
        }
    } else {
        let format = super::audio_format(format)?;
        args.extend(["-c:a".into(), format.audio_codec.unwrap_or("copy").into()]);
        if format.audio_bitrate.is_some() {
            let default = format
                .audio_bitrate
                .and_then(|value| value.trim_end_matches('k').parse().ok())
                .unwrap_or(192);
            let bitrate = option_u64(job.request, "bitrateKbps", default, 32, 512)?;
            args.extend(["-b:a".into(), format!("{bitrate}k").into()]);
        }
        args.extend(["-f".into(), format.muxer.into()]);
        format.extension
    };
    let stage = stage_directory(input)?;
    let mut result = produce(
        job,
        input,
        stage.path(),
        &[MediaInput::plain(input)],
        args,
        extension,
        "audio",
        info.duration,
        Some("file/audio"),
    )?;
    result.warnings = warnings;
    Ok(result)
}

// ---------------------------------------------------------------------------
// GIF / animated WebP
// ---------------------------------------------------------------------------

fn animation(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let request = job.request;
    let format = option_str(request, "format", "gif")?;
    if !matches!(format, "gif" | "webp") {
        return Err(format!("Unsupported animation format: {format}"));
    }
    let start = option_f64(request, "startSeconds", 0.0, 0.0, 7_200_000.0)?;
    let mut seconds = option_f64(request, "durationSeconds", 3.0, 0.1, 60.0)?;
    let fps = option_u64(request, "fps", 12, 1, 30)?;
    let width = option_u64(request, "width", 480, 64, 1280)?;
    let forever = match option_str(request, "loop", "forever")? {
        "forever" => true,
        "once" => false,
        other => return Err(format!("Unknown loop setting: {other}")),
    };
    let level = match option_str(request, "quality", "balanced")? {
        "high" => Quality::High,
        "balanced" => Quality::Balanced,
        "small" => Quality::Small,
        other => return Err(format!("Unknown quality preset: {other}")),
    };
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    info.require_video()?;
    if let Some(total) = info.duration {
        if start >= total {
            return Err(format!(
                "The start time is after the end of the video ({}).",
                clock(total)
            ));
        }
        seconds = seconds.min(total - start);
    }
    let base = format!("fps={fps},scale=min({width}\\,iw):-2:flags=lanczos");
    let mut args: Vec<OsString> = vec!["-t".into(), format_seconds(seconds).into()];
    if format == "gif" {
        let (colors, stats, dither) = match level {
            Quality::High => (256, "full", "sierra2_4a"),
            Quality::Balanced => (192, "diff", "bayer:bayer_scale=3"),
            Quality::Small => (96, "diff", "bayer:bayer_scale=5"),
        };
        args.extend([
            "-filter_complex".into(),
            format!(
                "[0:v:0]{base},split[a][b];[a]palettegen=max_colors={colors}:stats_mode={stats}[p];[b][p]paletteuse=dither={dither}:diff_mode=rectangle[out]"
            )
            .into(),
            "-map".into(),
            "[out]".into(),
            "-loop".into(),
            if forever { "0" } else { "-1" }.into(),
            "-f".into(),
            "gif".into(),
        ]);
    } else {
        let quality = match level {
            Quality::High => "90",
            Quality::Balanced => "75",
            Quality::Small => "55",
        };
        args.extend([
            "-map".into(),
            "0:v:0".into(),
            "-vf".into(),
            base.into(),
            "-an".into(),
            "-c:v".into(),
            "libwebp_anim".into(),
            "-lossless".into(),
            "0".into(),
            "-quality".into(),
            quality.into(),
            "-loop".into(),
            if forever { "0" } else { "1" }.into(),
            "-f".into(),
            "webp".into(),
        ]);
    }
    let stage = stage_directory(input)?;
    let options = if start > 0.0 {
        vec!["-ss".into(), format_seconds(start).into()]
    } else {
        Vec::new()
    };
    produce(
        job,
        input,
        stage.path(),
        &[MediaInput {
            options,
            path: input.to_path_buf(),
        }],
        args,
        format,
        "animation",
        Some(seconds),
        Some("file/image"),
    )
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

fn frames(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let request = job.request;
    let mode = option_str(request, "mode", "timestamp")?;
    let (extension, codec): (&str, Vec<OsString>) = match option_str(request, "imageFormat", "png")?
    {
        "png" => ("png", vec!["-c:v".into(), "png".into()]),
        "jpg" => (
            "jpg",
            vec!["-c:v".into(), "mjpeg".into(), "-q:v".into(), "2".into()],
        ),
        other => return Err(format!("Unsupported frame image format: {other}")),
    };
    let limit = option_u64(request, "maxFrames", 500, 1, MAX_FRAMES)?;
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    info.require_video()?;
    let total = info.duration;
    let check_time = |time: f64| match total {
        Some(total) if time >= total => Err(format!(
            "That time is after the end of the video ({}).",
            clock(total)
        )),
        _ => Ok(()),
    };
    let mut args: Vec<OsString> = vec!["-map".into(), "0:v:0".into(), "-an".into(), "-sn".into()];
    let (seek, span) = match mode {
        "timestamp" => {
            let time = option_f64(request, "timestampSeconds", 0.0, 0.0, 7_200_000.0)?;
            check_time(time)?;
            args.extend(["-frames:v".into(), "1".into()]);
            ((time > 0.0).then_some(time), Some(1.0))
        }
        "interval" | "everyFrames" => {
            let start = option_f64(request, "startSeconds", 0.0, 0.0, 7_200_000.0)?;
            check_time(start)?;
            let end = match request.options.get("endSeconds") {
                None | Some(Value::Null) => None,
                Some(_) => {
                    let end = option_f64(request, "endSeconds", 0.0, 0.1, 7_200_000.0)?;
                    if end <= start {
                        return Err("The end time must be after the start time".into());
                    }
                    Some(end)
                }
            };
            if let Some(end) = end {
                args.extend(["-t".into(), format_seconds(end - start).into()]);
            }
            if mode == "interval" {
                let interval = option_f64(request, "intervalSeconds", 1.0, 0.04, 86_400.0)?;
                args.extend([
                    "-vf".into(),
                    format!("fps=1/{}", format_seconds(interval)).into(),
                ]);
            } else {
                let step = option_u64(request, "frameStep", 30, 1, 100_000)?;
                args.extend(["-vf".into(), format!("select=not(mod(n\\,{step}))").into()]);
                args.extend(variable_frame_rate_args(job.provider));
            }
            // Stop one past the limit so an oversized request fails quickly.
            args.extend(["-frames:v".into(), (limit + 1).to_string().into()]);
            (
                (start > 0.0).then_some(start),
                end.or(total).map(|end| end - start),
            )
        }
        other => return Err(format!("Unknown frame extraction mode: {other}")),
    };
    let stage = stage_directory(input)?;
    let pattern = stage.path().join(format!("frame-%06d.{extension}"));
    args.extend(codec);
    args.extend([
        "-f".into(),
        "image2".into(),
        pattern.as_os_str().to_os_string(),
    ]);
    let options = seek
        .map(|time| vec!["-ss".into(), format_seconds(time).into()])
        .unwrap_or_default();
    run_ffmpeg(
        &job.provider.executable_path,
        &[MediaInput {
            options,
            path: input.to_path_buf(),
        }],
        args,
        stage.path(),
        span,
        job.cancelled,
        job.progress.clone(),
        "error",
    )?;
    let mut files: Vec<PathBuf> = fs::read_dir(stage.path())
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("frame-") && name.ends_with(extension))
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Err("No frames fell inside the chosen range".into());
    }
    if files.len() as u64 > limit {
        return Err(format!(
            "This range has more than {limit} frames. Narrow the range, sample less often, or raise Maximum frames."
        ));
    }
    let prefix = match request.options.get("prefix") {
        None | Some(Value::Null) => {
            sanitize_piece(&input.file_stem().unwrap_or_default().to_string_lossy())
        }
        Some(value) => sanitize_piece(value.as_str().ok_or("prefix must be text")?),
    };
    let prefix = if prefix.is_empty() {
        "video-frame".to_string()
    } else {
        prefix
    };
    let parent = input
        .parent()
        .ok_or("Source path has no parent directory")?;
    let mut outputs = Vec::with_capacity(files.len());
    let mut names = Vec::with_capacity(files.len());
    for (index, path) in files.iter().enumerate() {
        if job.cancelled.load(Ordering::Relaxed) {
            return Err(format!("Cancelled after saving {} frames", names.len()));
        }
        let name = format!("{prefix}-{:04}.{extension}", index + 1);
        let (published, selected) = publish_staged(path, parent, &name, job.grants, job.cancelled)
            .map_err(|error| format!("Saved {} frames, then stopped: {error}", names.len()))?;
        names.push(
            published
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        );
        outputs.push(selected.as_tool_value());
    }
    let count = names.len();
    let mut result = base_result(
        job.manifest,
        outputs,
        &if count == 1 {
            format!("Saved {}", names[0])
        } else {
            format!("Saved {count} frames")
        },
    );
    attach_provider(&mut result, job.provider);
    result.metadata.insert("outputNames".into(), json!(names));
    Ok(result)
}

// ---------------------------------------------------------------------------
// Subtitles
// ---------------------------------------------------------------------------

fn subtitle_extension(value: &str) -> Result<&'static str, String> {
    match value {
        "srt" => Ok("srt"),
        "vtt" => Ok("vtt"),
        "ass" | "ssa" => Ok("ass"),
        other => Err(format!("Unsupported subtitle output format: {other}")),
    }
}

fn subtitle_codec(value: &str) -> Result<&'static str, String> {
    match value {
        "srt" => Ok("srt"),
        "vtt" => Ok("webvtt"),
        "ass" | "ssa" => Ok("ass"),
        other => Err(format!("Unsupported subtitle output format: {other}")),
    }
}

fn subtitle_muxer(value: &str) -> Result<&'static str, String> {
    match value {
        "srt" => Ok("srt"),
        "vtt" => Ok("webvtt"),
        "ass" | "ssa" => Ok("ass"),
        other => Err(format!("Unsupported subtitle output format: {other}")),
    }
}

const SUBTITLE_EXTENSIONS: [&str; 5] = ["srt", "ass", "ssa", "vtt", "sub"];

fn is_subtitle_file(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            SUBTITLE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
}

/// Put the video first and the subtitle second, whatever order they were picked in.
fn video_and_subtitle(inputs: &[PathBuf]) -> Result<(&Path, &Path), String> {
    let (video, subtitle) = match inputs {
        [a, b] if is_subtitle_file(a) && !is_subtitle_file(b) => (b, a),
        [a, b] => (a, b),
        _ => return Err("Choose a video and one subtitle file".into()),
    };
    if !is_subtitle_file(subtitle) {
        return Err("Subtitle files must be SRT, ASS, SSA, VTT, or SUB".into());
    }
    Ok((video, subtitle))
}

/// Make `source` reachable under a fixed, filter-safe name inside the stage.
fn link_into(stage: &Path, source: &Path, name: &str) -> Result<PathBuf, String> {
    let target = stage.join(name);
    #[cfg(unix)]
    if std::os::unix::fs::symlink(source, &target).is_ok() {
        return Ok(target);
    }
    if fs::hard_link(source, &target).is_ok() {
        return Ok(target);
    }
    fs::copy(source, &target)
        .map(|_| target)
        .map_err(|error| format!("Cannot stage the file for subtitle rendering: {error}"))
}

fn subtitles(job: &Job, inputs: &[PathBuf]) -> Result<ToolResult, String> {
    match option_str(job.request, "operation", "extract")? {
        "extract" => extract_subtitles(job, &inputs[0]),
        "convert" => convert_subtitle(job, &inputs[0]),
        "remove" => remove_subtitles(job, &inputs[0]),
        "add" => add_subtitle(job, inputs),
        "burn" => burn_subtitles(job, inputs),
        other => Err(format!("Unknown subtitle operation: {other}")),
    }
}

fn extract_subtitles(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let format = option_str(job.request, "outputFormat", "srt")?;
    let (extension, codec, muxer) = (
        subtitle_extension(format)?,
        subtitle_codec(format)?,
        subtitle_muxer(format)?,
    );
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    if info.subtitles.is_empty() {
        return Err("This video has no embedded subtitle tracks".into());
    }
    let run = |stream: &StreamSummary,
               name: Option<String>,
               progress: Progress|
     -> Result<ToolResult, String> {
        let stage = stage_directory(input)?;
        let staged = stage.path().join(format!("output.{extension}"));
        run_ffmpeg(
            &job.provider.executable_path,
            &[MediaInput::plain(input)],
            vec![
                "-map".into(),
                format!("0:s:{}", stream.index).into(),
                "-c:s".into(),
                codec.into(),
                "-f".into(),
                muxer.into(),
                "-n".into(),
                staged.as_os_str().to_os_string(),
            ],
            stage.path(),
            info.duration,
            job.cancelled,
            progress,
            "error",
        )?;
        let name = match name {
            Some(name) => name,
            None => requested_output_name(job.request, input, "subtitles", extension)?,
        };
        publish_named(job, input, &staged, &name, Some("file/subtitle"))
    };
    if option_bool(job.request, "allStreams", false)? {
        let text: Vec<&StreamSummary> = info
            .subtitles
            .iter()
            .filter(|stream| !stream.bitmap)
            .collect();
        if text.is_empty() {
            return Err("Every subtitle track in this video is image-based (PGS/VobSub), so none can be saved as text. Use Burn to draw one onto the video.".into());
        }
        let count = text.len();
        let mut outputs = Vec::new();
        let mut names = Vec::new();
        let mut warnings: Vec<String> = info
            .subtitles
            .iter()
            .filter(|stream| stream.bitmap)
            .map(|stream| {
                format!(
                    "Skipped image-based track {} ({}).",
                    stream.index, stream.codec
                )
            })
            .collect();
        for (position, stream) in text.into_iter().enumerate() {
            let label = stream
                .language
                .as_deref()
                .map(sanitize_piece)
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| format!("track{}", stream.index));
            let name = super::default_output_name(input, &format!("subtitles-{label}"), extension);
            let progress = job.progress.clone();
            let result = run(
                stream,
                Some(name),
                Arc::new(move |fraction| progress((position as f64 + fraction) / count as f64)),
            )?;
            names.push(output_name(&result));
            warnings.extend(result.warnings);
            outputs.extend(result.outputs);
        }
        let mut result = base_result(
            job.manifest,
            outputs,
            &format!("Saved {} subtitle tracks as .{extension}", names.len()),
        );
        attach_provider(&mut result, job.provider);
        result.metadata.insert("outputNames".into(), json!(names));
        result.warnings = warnings;
        return Ok(result);
    }
    let index = option_u64(job.request, "streamIndex", 0, 0, 255)? as usize;
    let stream = info.subtitles.get(index).ok_or_else(|| {
        format!(
            "Subtitle track {index} doesn't exist; this video has {} (numbered from 0)",
            info.subtitles.len()
        )
    })?;
    if stream.bitmap {
        return Err(format!(
            "Subtitle track {index} is image-based ({}), so it can't be saved as text without OCR. Use Burn to draw it onto the video instead.",
            stream.codec
        ));
    }
    let mut result = run(stream, None, job.progress.clone())?;
    result.message = Some(format!(
        "Saved subtitle track {index} as {}",
        output_name(&result)
    ));
    Ok(result)
}

fn convert_subtitle(job: &Job, input: &Path) -> Result<ToolResult, String> {
    if !is_subtitle_file(input) {
        return Err("Choose an SRT, ASS, SSA, VTT, or SUB subtitle file to convert".into());
    }
    let format = option_str(job.request, "outputFormat", "srt")?;
    let extension = subtitle_extension(format)?;
    let stage = stage_directory(input)?;
    produce(
        job,
        input,
        stage.path(),
        &[MediaInput::plain(input)],
        vec![
            "-map".into(),
            "0:s:0".into(),
            "-c:s".into(),
            subtitle_codec(format)?.into(),
            "-f".into(),
            subtitle_muxer(format)?.into(),
        ],
        extension,
        "subtitles",
        None,
        Some("file/subtitle"),
    )
}

fn check_copyable(container: Container, info: &SourceInfo) -> Result<(), String> {
    if let Some(video) = &info.video
        && !container.holds_video(&video.codec)
    {
        return Err(format!(
            "{} can't hold this video's {} stream without re-encoding. Choose Matroska.",
            container.label(),
            video.codec
        ));
    }
    if let Some(audio) = info.first_audio()
        && !container.holds_audio(audio)
    {
        return Err(format!(
            "{} can't hold this video's {audio} audio without re-encoding. Choose Matroska.",
            container.label()
        ));
    }
    Ok(())
}

fn remove_subtitles(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let container = Container::parse(option_str(job.request, "videoFormat", "mkv")?)?;
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    info.require_video()?;
    if info.subtitles.is_empty() {
        return Err("This video has no subtitle tracks to remove".into());
    }
    check_copyable(container, &info)?;
    let mut args: Vec<OsString> = vec![
        "-map".into(),
        "0:v".into(),
        "-map".into(),
        "0:a?".into(),
        "-c".into(),
        "copy".into(),
    ];
    if matches!(container, Container::Mp4 | Container::Mov) {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args.extend(["-f".into(), container.muxer().into()]);
    let stage = stage_directory(input)?;
    let mut result = produce(
        job,
        input,
        stage.path(),
        &[MediaInput::plain(input)],
        args,
        container.extension(),
        "no-subtitles",
        info.duration,
        None,
    )?;
    result.message = Some(format!(
        "Removed {} subtitle track(s); saved {}",
        info.subtitles.len(),
        output_name(&result)
    ));
    Ok(result)
}

fn add_subtitle(job: &Job, inputs: &[PathBuf]) -> Result<ToolResult, String> {
    let (video, subtitle) = video_and_subtitle(inputs)?;
    let container = Container::parse(option_str(job.request, "videoFormat", "mkv")?)?;
    let info = probe(&job.provider.executable_path, video, job.cancelled)?;
    info.require_video()?;
    check_copyable(container, &info)?;
    let language = match option_str(job.request, "language", "")?.trim() {
        "" => None,
        value
            if (2..=3).contains(&value.len())
                && value.chars().all(|ch| ch.is_ascii_lowercase()) =>
        {
            Some(value.to_owned())
        }
        _ => return Err("Language must be a 2–3 letter code such as en or spa".into()),
    };
    let mut args: Vec<OsString> = vec!["-map".into(), "0:v".into(), "-map".into(), "0:a?".into()];
    let mut warnings = Vec::new();
    // Matroska keeps existing tracks; other containers would need them converted.
    let kept = if container == Container::Mkv {
        args.extend(["-map".into(), "0:s?".into()]);
        info.subtitles.len()
    } else {
        if !info.subtitles.is_empty() {
            warnings.push(format!(
                "{} existing subtitle track(s) were not carried over; use Matroska to keep them.",
                info.subtitles.len()
            ));
        }
        0
    };
    args.extend([
        "-map".into(),
        "1:s:0".into(),
        "-c:v".into(),
        "copy".into(),
        "-c:a".into(),
        "copy".into(),
    ]);
    // A general codec first, then the new track's own, so the specific one wins.
    match container {
        Container::Mp4 | Container::Mov => args.extend(["-c:s".into(), "mov_text".into()]),
        Container::Webm => args.extend(["-c:s".into(), "webvtt".into()]),
        Container::Mkv => {
            args.extend(["-c:s".into(), "copy".into()]);
            // MicroDVD (.sub) has no Matroska mapping; store it as SubRip.
            if subtitle
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("sub"))
            {
                args.extend([format!("-c:s:{kept}").into(), "srt".into()]);
            }
        }
    }
    if let Some(language) = &language {
        args.extend([
            format!("-metadata:s:s:{kept}").into(),
            format!("language={language}").into(),
        ]);
    }
    if matches!(container, Container::Mp4 | Container::Mov) {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args.extend(["-f".into(), container.muxer().into()]);
    let stage = stage_directory(video)?;
    let mut result = produce(
        job,
        video,
        stage.path(),
        &[MediaInput::plain(video), MediaInput::plain(subtitle)],
        args,
        container.extension(),
        "with-subtitles",
        info.duration,
        None,
    )?;
    result.warnings = warnings;
    Ok(result)
}

fn burn_subtitles(job: &Job, inputs: &[PathBuf]) -> Result<ToolResult, String> {
    let mut plan = plan(job.request, "videoFormat", "mkv", Quality::High)?;
    // Burning only changes the picture; keep the original audio when possible.
    if plan.audio == AudioMode::Encode {
        plan.audio = AudioMode::Copy;
    }
    let has = |capability: &str| {
        job.provider
            .capabilities
            .iter()
            .any(|have| have == capability)
    };
    let (video, external) = if inputs.len() == 2 {
        let (video, subtitle) = video_and_subtitle(inputs)?;
        (video, Some(subtitle))
    } else {
        (inputs[0].as_path(), None)
    };
    let info = probe(&job.provider.executable_path, video, job.cancelled)?;
    info.require_video()?;
    let stage = stage_directory(video)?;
    let mut args: Vec<OsString> = Vec::new();
    match external {
        Some(subtitle) => {
            let extension = subtitle
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("srt")
                .to_ascii_lowercase();
            let staged = stage.path().join(format!("external.{extension}"));
            fs::copy(subtitle, &staged)
                .map_err(|error| format!("Cannot stage the subtitle file: {error}"))?;
            args.extend([
                "-map".into(),
                "0:v:0".into(),
                "-vf".into(),
                format!("subtitles=filename=external.{extension}").into(),
            ]);
        }
        None => {
            if info.subtitles.is_empty() {
                return Err(
                    "This video has no embedded subtitles. Add a subtitle file to burn it in."
                        .into(),
                );
            }
            let index = option_u64(job.request, "streamIndex", 0, 0, 255)? as usize;
            let stream = info.subtitles.get(index).ok_or_else(|| {
                format!(
                    "Subtitle track {index} doesn't exist; this video has {} (numbered from 0)",
                    info.subtitles.len()
                )
            })?;
            if stream.bitmap {
                if !has("filter:overlay") {
                    return Err(provider_error(&["filter:overlay".into()]));
                }
                args.extend([
                    "-filter_complex".into(),
                    format!("[0:v:0][0:s:{index}]overlay=eof_action=pass[v]").into(),
                    "-map".into(),
                    "[v]".into(),
                ]);
            } else {
                if !has("filter:subtitles") {
                    return Err(provider_error(&["filter:subtitles".into()]));
                }
                let extension = video
                    .extension()
                    .and_then(|value| value.to_str())
                    .filter(|value| value.chars().all(|ch| ch.is_ascii_alphanumeric()))
                    .unwrap_or("video");
                let linked = format!("source.{extension}");
                link_into(stage.path(), video, &linked)?;
                args.extend([
                    "-map".into(),
                    "0:v:0".into(),
                    "-vf".into(),
                    format!("subtitles=filename={linked}:si={index}").into(),
                ]);
            }
        }
    }
    audio_maps(&plan, &info, &mut args);
    let mut warnings = Vec::new();
    args.extend(plan.video_args());
    args.extend(plan.audio_args(info.first_audio(), &mut warnings));
    args.extend(plan.container_args());
    let mut result = produce(
        job,
        video,
        stage.path(),
        &[MediaInput::plain(video)],
        args,
        plan.container.extension(),
        "subtitled",
        info.duration,
        None,
    )?;
    result.warnings = warnings;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Previews and estimates for the visual editors
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewFrame {
    pub time_seconds: f64,
    pub data_url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoPreview {
    pub duration_seconds: Option<f64>,
    pub width: u64,
    pub height: u64,
    pub frame_rate: Option<f64>,
    pub video_codec: Option<String>,
    pub source_bytes: Option<u64>,
    pub audio: Vec<StreamSummary>,
    pub subtitles: Vec<StreamSummary>,
    pub thumbnails: Vec<PreviewFrame>,
    pub frame: Option<PreviewFrame>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEstimate {
    pub source_bytes: Option<u64>,
    pub duration_seconds: f64,
    pub estimated_bytes: u64,
    /// `target` when the size was requested directly, `sample` when measured.
    pub method: String,
    pub sampled_seconds: f64,
    pub warnings: Vec<String>,
}

/// Provider discovery runs several FFmpeg probes, so editor previews reuse a
/// recent result instead of repeating it on every scrub.
pub(super) fn cached_provider() -> Result<ProviderInfo, String> {
    static CACHE: Mutex<Option<(ProviderInfo, Instant)>> = Mutex::new(None);
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((provider, at)) = cache.as_ref()
        && at.elapsed() < Duration::from_secs(60)
        && provider.executable_path.is_file()
    {
        return Ok(provider.clone());
    }
    let provider = discover_ffmpeg(None)
        .into_iter()
        .find(|provider| provider.compatible)
        .ok_or_else(|| provider_error(&["probe:streams".into()]))?;
    *cache = Some((provider.clone(), Instant::now()));
    Ok(provider)
}

/// Thumbnails across the clip plus an optional full frame for the crop editor.
pub fn video_preview(
    grants: &FileGrants,
    token: &str,
    thumbnails: u32,
    frame_at: Option<f64>,
    cancelled: &AtomicBool,
) -> Result<VideoPreview, String> {
    let path = grants.resolve(token).map_err(|error| error.to_string())?;
    let provider = cached_provider()?;
    let ffmpeg = &provider.executable_path;
    let info = probe(ffmpeg, &path, cancelled)?;
    let (width, height) = info
        .video
        .as_ref()
        .map(VideoStream::display_size)
        .unwrap_or((0, 0));
    let mut preview = VideoPreview {
        duration_seconds: info.duration,
        width,
        height,
        frame_rate: info.video.as_ref().and_then(VideoStream::fps),
        video_codec: info.video.as_ref().map(|video| video.codec.clone()),
        source_bytes: info.bytes,
        audio: info.audio.clone(),
        subtitles: info.subtitles.clone(),
        thumbnails: Vec::new(),
        frame: None,
    };
    if info.video.is_none() {
        return Ok(preview);
    }
    let last = info.duration.map_or(0.0, |total| (total - 0.05).max(0.0));
    let count = thumbnails.min(16);
    // (time, output height)
    let mut shots: Vec<(f64, u32)> = match info.duration {
        Some(total) => (0..count)
            .map(|index| (total * (index as f64 + 0.5) / count as f64, 72))
            .collect(),
        None if count > 0 => vec![(0.0, 72)],
        None => Vec::new(),
    };
    let wants_frame = frame_at.is_some();
    if let Some(time) = frame_at {
        // A negative time asks for the middle of the clip.
        let time = if time < 0.0 { last / 2.0 } else { time };
        shots.push((time.clamp(0.0, last), 540));
    }
    if shots.is_empty() {
        return Ok(preview);
    }
    let stage = tempfile::Builder::new()
        .prefix("arcade-video-preview-")
        .tempdir()
        .map_err(|error| format!("Cannot create a preview folder: {error}"))?;
    let inputs: Vec<MediaInput> = shots
        .iter()
        .map(|(time, _)| MediaInput {
            options: vec!["-ss".into(), format_seconds(*time).into()],
            path: path.clone(),
        })
        .collect();
    let mut args: Vec<OsString> = Vec::new();
    for (index, (_, rows)) in shots.iter().enumerate() {
        args.extend([
            "-map".into(),
            format!("{index}:v:0").into(),
            "-frames:v".into(),
            "1".into(),
            "-vf".into(),
            format!("scale=-2:min({rows}\\,ih)").into(),
            "-c:v".into(),
            "mjpeg".into(),
            "-q:v".into(),
            "5".into(),
            "-update".into(),
            "1".into(),
            "-f".into(),
            "image2".into(),
            "-y".into(),
            stage
                .path()
                .join(format!("shot-{index}.jpg"))
                .into_os_string(),
        ]);
    }
    run_ffmpeg(
        ffmpeg,
        &inputs,
        args,
        stage.path(),
        None,
        cancelled,
        Arc::new(|_| {}),
        "error",
    )?;
    for (index, (time, _)) in shots.iter().enumerate() {
        let Ok(bytes) = fs::read(stage.path().join(format!("shot-{index}.jpg"))) else {
            continue;
        };
        let frame = PreviewFrame {
            time_seconds: *time,
            data_url: format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)),
        };
        if wants_frame && index == shots.len() - 1 {
            preview.frame = Some(frame);
        } else {
            preview.thumbnails.push(frame);
        }
    }
    Ok(preview)
}

/// Predict the compressed size. Target-size mode is exact by construction;
/// the other modes encode a few short samples with the real settings.
pub fn estimate_video_output(
    grants: &FileGrants,
    token: &str,
    options: Value,
    cancelled: &AtomicBool,
) -> Result<VideoEstimate, String> {
    let path = grants.resolve(token).map_err(|error| error.to_string())?;
    let request = ToolRequest {
        tool_id: "arcade.video.compress".into(),
        inputs: Vec::new(),
        options,
    };
    let plan = plan(&request, "format", "mp4", Quality::Small)?;
    let provider = cached_provider()?;
    let mut required = Vec::new();
    plan.capabilities(&mut required);
    if let Some(missing) = required
        .iter()
        .find(|capability| !provider.capabilities.contains(capability))
    {
        return Err(provider_error(std::slice::from_ref(missing)));
    }
    let ffmpeg = &provider.executable_path;
    let info = probe(ffmpeg, &path, cancelled)?;
    info.require_video()?;
    let duration = info
        .duration
        .ok_or("Could not read the video duration needed for an estimate")?;
    let settings = compress_settings(&request, &plan, &info)?;
    let mut warnings = settings.warnings.clone();
    if option_str(&request, "mode", "quality")? == "targetSize" {
        let megabytes = option_f64(&request, "targetSizeMb", 100.0, 0.1, 100_000.0)?;
        return Ok(VideoEstimate {
            source_bytes: info.bytes,
            duration_seconds: duration,
            estimated_bytes: (megabytes * 1024.0 * 1024.0) as u64,
            method: "target".into(),
            sampled_seconds: 0.0,
            warnings,
        });
    }
    let samples: Vec<(f64, f64)> = if duration <= 15.0 {
        vec![(0.0, duration)]
    } else {
        [0.15, 0.5, 0.8]
            .iter()
            .map(|position| ((duration * position).min(duration - 4.0), 4.0))
            .collect()
    };
    let stage = tempfile::Builder::new()
        .prefix("arcade-video-estimate-")
        .tempdir()
        .map_err(|error| format!("Cannot create an estimate folder: {error}"))?;
    let mut bytes = 0u64;
    let mut seconds = 0.0;
    for (index, (start, length)) in samples.iter().enumerate() {
        let output = stage.path().join(format!("sample-{index}.mkv"));
        let mut args: Vec<OsString> = vec![
            "-t".into(),
            format_seconds(*length).into(),
            "-map".into(),
            "0:v:0".into(),
        ];
        audio_maps(&plan, &info, &mut args);
        args.extend(compress_stream_args(&plan, &settings, &info, &mut warnings));
        args.extend([
            "-f".into(),
            "matroska".into(),
            "-y".into(),
            output.clone().into_os_string(),
        ]);
        run_ffmpeg(
            ffmpeg,
            &[MediaInput {
                options: vec!["-ss".into(), format_seconds(*start).into()],
                path: path.clone(),
            }],
            args,
            stage.path(),
            None,
            cancelled,
            Arc::new(|_| {}),
            "error",
        )?;
        bytes += fs::metadata(&output).map(|meta| meta.len()).unwrap_or(0);
        seconds += length;
    }
    if bytes == 0 || seconds <= 0.0 {
        return Err("The sample encode produced no output to measure".into());
    }
    warnings.dedup();
    Ok(VideoEstimate {
        source_bytes: info.bytes,
        duration_seconds: duration,
        estimated_bytes: (bytes as f64 / seconds * duration) as u64,
        method: "sample".into(),
        sampled_seconds: seconds,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grants::SelectedFile;
    use arcade_contract::ResultStatus;

    struct Fixture {
        runtime: crate::Arcade,
        provider: ProviderInfo,
        _dir: tempfile::TempDir,
        dir: PathBuf,
    }

    impl Fixture {
        fn new() -> Option<Self> {
            let provider = discover_ffmpeg(None)
                .into_iter()
                .find(|provider| provider.compatible)?;
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().to_path_buf();
            Some(Self {
                runtime: crate::Arcade::in_memory().unwrap(),
                provider,
                _dir: dir,
                dir: path,
            })
        }

        fn has(&self, capabilities: &[&str]) -> bool {
            capabilities.iter().all(|capability| {
                self.provider
                    .capabilities
                    .iter()
                    .any(|have| have == capability)
            })
        }

        fn ffmpeg(&self, args: &[&str]) {
            let mut full: Vec<OsString> = ["-nostdin", "-hide_banner", "-loglevel", "error", "-y"]
                .iter()
                .map(OsString::from)
                .collect();
            full.extend(args.iter().map(OsString::from));
            let output = process::run(
                &ProcessSpec {
                    executable: self.provider.executable_path.clone(),
                    args: full,
                    current_dir: Some(self.dir.clone()),
                    timeout: Duration::from_secs(60),
                    output_limit: 1024 * 1024,
                },
                &AtomicBool::new(false),
            )
            .unwrap();
            assert!(
                output.status.success(),
                "fixture: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        /// A 2-second 24 fps clip with a keyframe every 0.5 s.
        fn clip(&self, name: &str, size: &str, audio: bool) -> SelectedFile {
            let mut args = vec![
                "-f",
                "lavfi",
                "-i",
                Box::leak(format!("testsrc2=size={size}:rate=24:duration=2").into_boxed_str()),
            ];
            if audio {
                args.extend([
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000:duration=2",
                ]);
            }
            args.extend(["-c:v", "libx264", "-g", "12", "-pix_fmt", "yuv420p"]);
            if audio {
                args.extend(["-c:a", "aac", "-ac", "2"]);
            }
            args.push(name);
            self.ffmpeg(&args);
            self.runtime.grants().grant(&self.dir.join(name)).unwrap()
        }

        fn subtitle_file(&self, name: &str) -> SelectedFile {
            let path = self.dir.join(name);
            fs::write(
                &path,
                "1\n00:00:00,100 --> 00:00:01,200\nHello there\n\n2\n00:00:01,300 --> 00:00:01,900\nSecond line\n",
            )
            .unwrap();
            self.runtime.grants().grant(&path).unwrap()
        }

        fn run(
            &self,
            id: &str,
            inputs: &[&SelectedFile],
            options: Value,
        ) -> Result<ToolResult, String> {
            let manifest = self
                .runtime
                .list_tools()
                .into_iter()
                .find(|tool| tool.id == id)
                .unwrap();
            super::super::execute(
                &manifest,
                &ToolRequest {
                    tool_id: id.into(),
                    inputs: inputs.iter().map(|file| file.as_tool_value()).collect(),
                    options,
                },
                self.runtime.grants(),
                &AtomicBool::new(false),
            )
        }

        fn ok(&self, id: &str, inputs: &[&SelectedFile], options: Value) -> ToolResult {
            let result = self
                .run(id, inputs, options.clone())
                .unwrap_or_else(|error| panic!("{id} {options}: {error}"));
            assert_eq!(
                result.status,
                ResultStatus::Success,
                "{id}: {:?}",
                result.message
            );
            assert!(!result.outputs.is_empty(), "{id} returned no output");
            result
        }

        fn info(&self, result: &ToolResult, index: usize) -> SourceInfo {
            let path = self
                .runtime
                .grants()
                .resolve(&result.outputs[index].value)
                .unwrap();
            probe(
                &self.provider.executable_path,
                &path,
                &AtomicBool::new(false),
            )
            .unwrap()
        }
    }

    #[test]
    fn plan_rejects_codecs_the_container_cannot_hold() {
        let request = |options: Value| ToolRequest {
            tool_id: "arcade.video.convert".into(),
            inputs: Vec::new(),
            options,
        };
        assert!(
            plan(
                &request(json!({"format": "webm", "videoCodec": "h264"})),
                "format",
                "mp4",
                Quality::Balanced
            )
            .is_err()
        );
        assert!(
            plan(
                &request(json!({"format": "mp4", "videoCodec": "prores"})),
                "format",
                "mp4",
                Quality::Balanced
            )
            .is_err()
        );
        let prores = plan(
            &request(json!({"format": "mov", "videoCodec": "prores", "quality": "high"})),
            "format",
            "mp4",
            Quality::Balanced,
        )
        .unwrap();
        assert_eq!(prores.crf, 3);
        // Saved pipelines may still send a CRF number as `quality`.
        let legacy = plan(
            &request(json!({"quality": 30})),
            "format",
            "mp4",
            Quality::Balanced,
        )
        .unwrap();
        assert_eq!((legacy.codec, legacy.crf), (VideoCodec::H264, 30));
        let preset = plan(
            &request(json!({"quality": "small", "format": "webm"})),
            "format",
            "mp4",
            Quality::Balanced,
        )
        .unwrap();
        assert_eq!((preset.codec, preset.crf), (VideoCodec::Vp9, 40));
        assert_eq!(centered_crop(1920, 1080, (1, 1)), (1080, 1080, 420, 0));
        assert_eq!(centered_crop(1080, 1920, (16, 9)), (1080, 606, 0, 657));
    }

    #[test]
    fn installed_ffmpeg_runs_every_video_tool() {
        let Some(fx) = Fixture::new() else { return };
        if !fx.has(&["encoder:libx264", "encoder:aac", "mux:mp4", "mux:matroska"]) {
            return;
        }
        let a = fx.clip("a.mp4", "160x96", true);
        let b = fx.clip("b.mp4", "160x96", true);
        let c = fx.clip("c.mp4", "128x128", false);

        // Convert: presets, codec choice, resolution cap, batch.
        let converted = fx.ok(
            "arcade.video.convert",
            &[&a],
            json!({"quality": "small", "maxHeight": "480"}),
        );
        assert_eq!(converted.outputs[0].mime, "file/video");
        if fx.has(&["encoder:libvpx-vp9", "encoder:libopus", "mux:webm"]) {
            let webm = fx.ok("arcade.video.convert", &[&a], json!({"format": "webm"}));
            assert_eq!(fx.info(&webm, 0).video.unwrap().codec, "vp9");
        }
        if fx.has(&["encoder:libx265"]) {
            let hevc = fx.ok(
                "arcade.video.convert",
                &[&a],
                json!({"videoCodec": "h265", "audio": "copy"}),
            );
            assert_eq!(fx.info(&hevc, 0).video.unwrap().codec, "hevc");
        }
        let batch = fx.ok(
            "arcade.video.convert",
            &[&a, &b],
            json!({"outputName": "ignored"}),
        );
        assert_eq!(batch.outputs.len(), 2);
        assert_eq!(batch.metadata["outputNames"].as_array().unwrap().len(), 2);

        // Trim: copy on keyframes, re-encode off them, snap in fast mode.
        let copied = fx.ok(
            "arcade.video.trim",
            &[&a],
            json!({"startSeconds": 0.5, "endSeconds": 1.5}),
        );
        assert_eq!(copied.metadata["method"], "copy");
        let precise = fx.ok(
            "arcade.video.trim",
            &[&a],
            json!({"startSeconds": 0.3, "endSeconds": 1.2}),
        );
        assert_eq!(precise.metadata["method"], "reencode");
        assert!(!precise.warnings.is_empty());
        let duration = fx.info(&precise, 0).duration.unwrap();
        assert!(
            (duration - 0.9).abs() < 0.15,
            "precise trim lasted {duration}"
        );
        let fast = fx.ok(
            "arcade.video.trim",
            &[&a],
            json!({"mode": "fast", "startSeconds": 0.7}),
        );
        assert_eq!(fast.metadata["method"], "copy");
        assert!(fast.warnings[0].contains("keyframe"));
        assert!(
            fx.run("arcade.video.trim", &[&a], json!({"startSeconds": 9.0}))
                .is_err()
        );

        // Crop: aspect preset, flip, and a bounds check.
        let square = fx.ok(
            "arcade.video.crop",
            &[&a],
            json!({"aspect": "1:1", "flip": "horizontal"}),
        );
        let video = fx.info(&square, 0).video.unwrap();
        assert_eq!((video.width, video.height), (96, 96));
        let error = fx
            .run(
                "arcade.video.crop",
                &[&a],
                json!({"cropWidth": 200, "cropHeight": 50}),
            )
            .unwrap_err();
        assert!(error.contains("goes past"), "{error}");

        // Compress: quality, two-pass target size, and the size estimate.
        fx.ok("arcade.video.compress", &[&a], json!({"mode": "quality"}));
        let targeted = fx.ok(
            "arcade.video.compress",
            &[&a],
            json!({"mode": "targetSize", "targetSizeMb": 0.1, "audioKbps": 64}),
        );
        assert_eq!(targeted.metadata["twoPass"], true);
        let too_small = fx
            .run(
                "arcade.video.compress",
                &[&a],
                json!({"mode": "targetSize", "targetSizeMb": 0.1, "audioKbps": 512}),
            )
            .unwrap_err();
        assert!(too_small.contains("too small"), "{too_small}");
        let estimate = estimate_video_output(
            fx.runtime.grants(),
            &a.token,
            json!({"mode": "quality"}),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(estimate.method, "sample");
        assert!(estimate.estimated_bytes > 0);
        let target = estimate_video_output(
            fx.runtime.grants(),
            &a.token,
            json!({"mode": "targetSize", "targetSizeMb": 5}),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(target.method, "target");

        // Join: lossless when clips match, transcode (with silence) when not.
        let joined = fx.ok("arcade.video.join", &[&a, &b], json!({}));
        assert_eq!(joined.metadata["method"], "copy");
        assert!((fx.info(&joined, 0).duration.unwrap() - 4.0).abs() < 0.2);
        let mixed = fx.ok("arcade.video.join", &[&a, &c], json!({}));
        assert_eq!(mixed.metadata["method"], "reencode");
        assert!(mixed.warnings[0].contains("clip 2"), "{:?}", mixed.warnings);
        let mixed_info = fx.info(&mixed, 0);
        assert_eq!(
            mixed_info.audio.len(),
            1,
            "silence fills the clip without audio"
        );

        // Extract audio: lossless copy, encoded formats, missing tracks.
        let original = fx.ok(
            "arcade.video.extract-audio",
            &[&a],
            json!({"format": "copy"}),
        );
        assert_eq!(original.outputs[0].mime, "file/audio");
        assert_eq!(fx.info(&original, 0).audio[0].codec, "aac");
        if fx.has(&["encoder:libmp3lame", "mux:mp3"]) {
            fx.ok(
                "arcade.video.extract-audio",
                &[&a],
                json!({"format": "mp3", "bitrateKbps": 128}),
            );
        }
        assert!(
            fx.run(
                "arcade.video.extract-audio",
                &[&a],
                json!({"format": "copy", "track": 3})
            )
            .is_err()
        );
        assert!(
            fx.run(
                "arcade.video.extract-audio",
                &[&c],
                json!({"format": "copy"})
            )
            .is_err()
        );

        // GIF / WebP with loop and quality controls.
        if fx.has(&["encoder:gif", "filter:palettegen"]) {
            let gif = fx.ok("arcade.video.gif", &[&a], json!({"startSeconds": 0.5, "durationSeconds": 0.5, "fps": 8, "width": 96, "loop": "once", "quality": "small"}));
            assert_eq!(gif.outputs[0].mime, "file/image");
        }
        if fx.has(&["encoder:libwebp_anim", "mux:webp"]) {
            fx.ok(
                "arcade.video.gif",
                &[&a],
                json!({"format": "webp", "durationSeconds": 0.5, "width": 96}),
            );
        }

        // Frames: one timestamp, every N frames as JPEG, intervals, and limits.
        fx.ok(
            "arcade.video.frames",
            &[&a],
            json!({"mode": "timestamp", "timestampSeconds": 1.0}),
        );
        if fx.has(&["encoder:mjpeg", "filter:select"]) {
            let every = fx.ok(
                "arcade.video.frames",
                &[&a],
                json!({"mode": "everyFrames", "frameStep": 12, "imageFormat": "jpg"}),
            );
            assert_eq!(every.outputs.len(), 4);
        }
        let sampled = fx.ok("arcade.video.frames", &[&a], json!({"mode": "interval", "intervalSeconds": 0.5, "startSeconds": 0.5, "endSeconds": 1.5}));
        assert!(
            (2..=3).contains(&sampled.outputs.len()),
            "{}",
            sampled.outputs.len()
        );
        let limited = fx
            .run(
                "arcade.video.frames",
                &[&a],
                json!({"mode": "everyFrames", "frameStep": 1, "maxFrames": 5}),
            )
            .unwrap_err();
        assert!(limited.contains("more than 5"), "{limited}");

        // Previews for the timeline and crop editors.
        let preview = video_preview(
            fx.runtime.grants(),
            &a.token,
            4,
            Some(1.0),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(preview.thumbnails.len(), 4);
        assert!(preview.frame.is_some());
        assert_eq!((preview.width, preview.height), (160, 96));
        assert_eq!(preview.audio.len(), 1);
    }

    #[test]
    fn installed_ffmpeg_handles_subtitle_tracks() {
        let Some(fx) = Fixture::new() else { return };
        if !fx.has(&[
            "encoder:libx264",
            "encoder:aac",
            "encoder:srt",
            "mux:srt",
            "mux:matroska",
            "mux:mp4",
            "encoder:mov_text",
        ]) {
            return;
        }
        let video = fx.clip("plain.mp4", "160x96", true);
        let srt = fx.subtitle_file("captions.srt");

        // Add a track (MP4 converts it to mov_text) and then a Matroska copy.
        let with_mp4 = fx.ok(
            "arcade.video.subtitles",
            &[&srt, &video],
            json!({"operation": "add", "videoFormat": "mp4", "language": "en"}),
        );
        let mp4_info = fx.info(&with_mp4, 0);
        assert_eq!(mp4_info.subtitles.len(), 1);
        assert_eq!(mp4_info.subtitles[0].codec, "mov_text");
        let with_mkv = fx.ok(
            "arcade.video.subtitles",
            &[&video, &srt],
            json!({"operation": "add", "videoFormat": "mkv"}),
        );
        let mkv_token = fx
            .runtime
            .grants()
            .resolve(&with_mkv.outputs[0].value)
            .unwrap();
        let mkv = fx.runtime.grants().grant(&mkv_token).unwrap();

        // Extract one track and all tracks; reject a track that isn't there.
        let extracted = fx.ok(
            "arcade.video.subtitles",
            &[&mkv],
            json!({"operation": "extract", "outputFormat": "vtt"}),
        );
        assert_eq!(extracted.outputs[0].mime, "file/subtitle");
        let all = fx.ok(
            "arcade.video.subtitles",
            &[&mkv],
            json!({"operation": "extract", "allStreams": true}),
        );
        assert_eq!(all.outputs.len(), 1);
        assert!(
            fx.run(
                "arcade.video.subtitles",
                &[&mkv],
                json!({"operation": "extract", "streamIndex": 4})
            )
            .is_err()
        );

        // Convert a subtitle file, then remove tracks from the video.
        fx.ok(
            "arcade.video.subtitles",
            &[&srt],
            json!({"operation": "convert", "outputFormat": "ass"}),
        );
        let removed = fx.ok(
            "arcade.video.subtitles",
            &[&mkv],
            json!({"operation": "remove"}),
        );
        assert!(fx.info(&removed, 0).subtitles.is_empty());
        assert!(
            fx.run(
                "arcade.video.subtitles",
                &[&video],
                json!({"operation": "remove"})
            )
            .is_err()
        );

        // Burn an external file and an embedded track into the picture.
        if fx.has(&["filter:subtitles"]) {
            let burned = fx.ok(
                "arcade.video.subtitles",
                &[&video, &srt],
                json!({"operation": "burn", "videoFormat": "mp4"}),
            );
            assert!(fx.info(&burned, 0).subtitles.is_empty());
            fx.ok(
                "arcade.video.subtitles",
                &[&mkv],
                json!({"operation": "burn", "streamIndex": 0}),
            );
        }
    }
}
