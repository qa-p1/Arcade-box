//! Audio actions: one output-format model shared by every tool that writes
//! audio, stream copy where it is lossless, two-pass loudness normalization,
//! silence analysis, tag editing, and the previews behind the waveform editors.
//!
//! Options arrive from standard forms, saved pipelines, and the CLI, so every
//! value is validated here. Provider calls use argument vectors only.

use super::job::{Job, MAX_BATCH, Progress, batch, clock, output_name, produce, publish_named};
use super::video::{cached_provider, copy_target};
use super::{
    MediaInput, attach_provider, base_result, ffprobe_json, format_seconds, option_bool,
    option_f64, option_str, option_u64, requested_output_name, run_ffmpeg, sanitize_piece,
    stage_directory, validate_range,
};
use crate::{artifacts::validate_portable_filename, grants::FileGrants};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ToolValue};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

mod enhance;

/// Tools that accept several files and process each one independently.
const BATCH_TOOLS: [&str; 3] = [
    "arcade.audio.convert",
    "arcade.audio.normalize",
    "arcade.audio.denoise",
];
const MAX_SEGMENTS: usize = 500;
const OPUS_RATES: [u64; 5] = [8000, 12000, 16000, 24000, 48000];
/// Form keys and the FFmpeg metadata keys they edit.
const TAG_FIELDS: [(&str, &str); 10] = [
    ("title", "title"),
    ("artist", "artist"),
    ("album", "album"),
    ("albumArtist", "album_artist"),
    ("track", "track"),
    ("disc", "disc"),
    ("year", "date"),
    ("genre", "genre"),
    ("composer", "composer"),
    ("comment", "comment"),
];

// ---------------------------------------------------------------------------
// Output format
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Encoding {
    Mp3,
    M4a,
    Flac,
    Wav,
    Ogg,
    Opus,
}

impl Encoding {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "mp3" => Ok(Self::Mp3),
            "m4a" => Ok(Self::M4a),
            "flac" => Ok(Self::Flac),
            "wav" => Ok(Self::Wav),
            "ogg" => Ok(Self::Ogg),
            "opus" => Ok(Self::Opus),
            other => Err(format!("Unsupported audio format: {other}")),
        }
    }

    /// The encoding that keeps a source in its own format.
    fn of_codec(codec: &str) -> Option<Self> {
        match codec {
            "mp3" => Some(Self::Mp3),
            "aac" => Some(Self::M4a),
            "flac" => Some(Self::Flac),
            "vorbis" => Some(Self::Ogg),
            "opus" => Some(Self::Opus),
            codec if codec.starts_with("pcm_") => Some(Self::Wav),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Mp3 => "MP3",
            Self::M4a => "M4A",
            Self::Flac => "FLAC",
            Self::Wav => "WAV",
            Self::Ogg => "Ogg Vorbis",
            Self::Opus => "Opus",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::Flac => "flac",
            Self::Wav => "wav",
            Self::Ogg => "ogg",
            Self::Opus => "opus",
        }
    }

    fn muxer(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::M4a => "ipod",
            Self::Flac => "flac",
            Self::Wav => "wav",
            Self::Ogg | Self::Opus => "ogg",
        }
    }

    fn encoder(self) -> &'static str {
        match self {
            Self::Mp3 => "libmp3lame",
            Self::M4a => "aac",
            Self::Flac => "flac",
            Self::Wav => "pcm_s16le",
            Self::Ogg => "libvorbis",
            Self::Opus => "libopus",
        }
    }

    fn lossy(self) -> bool {
        matches!(self, Self::Mp3 | Self::M4a | Self::Ogg | Self::Opus)
    }

    fn default_kbps(self) -> u64 {
        if self == Self::Opus { 128 } else { 192 }
    }

    fn max_kbps(self) -> u64 {
        if self == Self::Mp3 { 320 } else { 512 }
    }

    fn holds_cover(self) -> bool {
        matches!(self, Self::Mp3 | Self::M4a | Self::Flac)
    }

    /// The nearest sample rate this encoder accepts.
    fn fit_rate(self, rate: u64) -> u64 {
        match self {
            Self::Opus => 48_000,
            Self::Mp3 => rate.min(48_000),
            _ => rate,
        }
    }

    fn capabilities(self, required: &mut Vec<String>) {
        required.push(format!("encoder:{}", self.encoder()));
        required.push(format!("mux:{}", self.muxer()));
    }
}

/// The explicit encoding a request asks for, or `None` for "same as source".
fn requested_encoding(request: &ToolRequest, default: &str) -> Result<Option<Encoding>, String> {
    match option_str(request, "format", default)? {
        "same" => Ok(None),
        value => Encoding::parse(value).map(Some),
    }
}

/// What an audio tool writes and how.
struct Output {
    encoding: Encoding,
    kbps: Option<u64>,
    sample_rate: Option<u64>,
    channels: Option<u64>,
}

impl Output {
    fn resolve(
        request: &ToolRequest,
        default: &str,
        stream: &AudioStream,
    ) -> Result<(Self, Vec<String>), String> {
        let mut warnings = Vec::new();
        let (encoding, same) = match requested_encoding(request, default)? {
            Some(encoding) => (encoding, false),
            None => match Encoding::of_codec(&stream.codec) {
                Some(encoding) => (encoding, true),
                None => {
                    warnings.push(format!(
                        "{} audio can't be written back in its own format, so it was saved as lossless FLAC.",
                        stream.codec
                    ));
                    (Encoding::Flac, true)
                }
            },
        };
        let kbps = if !encoding.lossy() {
            None
        } else if same {
            // Keep roughly the source's bitrate instead of inflating or starving it.
            let source = stream.bitrate.map_or(encoding.default_kbps(), |bits| {
                ((bits as f64 / 1000.0 / 8.0).round() as u64) * 8
            });
            Some(source.clamp(64, encoding.max_kbps()))
        } else {
            let kbps = option_u64(request, "bitrateKbps", encoding.default_kbps(), 32, 512)?;
            if kbps > encoding.max_kbps() {
                return Err(format!(
                    "{} tops out at {} kb/s",
                    encoding.label(),
                    encoding.max_kbps()
                ));
            }
            Some(kbps)
        };
        let sample_rate = match request.options.get("sampleRate") {
            None | Some(Value::Null) => None,
            Some(Value::String(value)) if value.is_empty() || value == "original" => None,
            Some(Value::String(value)) => Some(
                value
                    .parse::<u64>()
                    .map_err(|_| "sampleRate must be a number of hertz".to_string())?,
            ),
            Some(value) => Some(value.as_u64().ok_or("sampleRate must be a whole number")?),
        };
        if let Some(rate) = sample_rate {
            if !(8_000..=192_000).contains(&rate) {
                return Err("sampleRate must be between 8000 and 192000 Hz".into());
            }
            if encoding == Encoding::Opus && !OPUS_RATES.contains(&rate) {
                return Err("Opus supports 8, 12, 16, 24, or 48 kHz".into());
            }
            if encoding == Encoding::Mp3 && rate > 48_000 {
                return Err("MP3 supports sample rates up to 48 kHz".into());
            }
        }
        let channels = match option_str(request, "channels", "original")? {
            "original" | "" => None,
            "mono" => Some(1),
            "stereo" => Some(2),
            other => return Err(format!("Unknown channel setting: {other}")),
        };
        Ok((
            Self {
                encoding,
                kbps,
                sample_rate,
                channels,
            },
            warnings,
        ))
    }

    /// Encoder arguments. `restore_rate` puts back the source rate after a
    /// filter such as loudnorm that resamples internally.
    fn codec_args(&self, stream: &AudioStream, restore_rate: bool) -> Vec<OsString> {
        let encoder = if self.encoding == Encoding::Wav && stream.bits.is_some_and(|bits| bits > 16)
        {
            "pcm_s24le"
        } else {
            self.encoding.encoder()
        };
        let mut args: Vec<OsString> = vec!["-c:a".into(), encoder.into()];
        if let Some(kbps) = self.kbps {
            args.extend(["-b:a".into(), format!("{kbps}k").into()]);
        }
        let rate = self.sample_rate.or_else(|| {
            (restore_rate || self.encoding == Encoding::Opus || stream.sample_rate > 48_000)
                .then(|| self.encoding.fit_rate(stream.sample_rate.max(8_000)))
        });
        if let Some(rate) = rate {
            args.extend(["-ar".into(), rate.to_string().into()]);
        }
        if let Some(channels) = self.channels {
            args.extend(["-ac".into(), channels.to_string().into()]);
        }
        if self.encoding == Encoding::Mp3 {
            args.extend(["-id3v2_version".into(), "3".into()]);
        }
        args.extend(["-f".into(), self.encoding.muxer().into()]);
        args
    }

    fn describe(&self) -> String {
        let mut parts = vec![self.encoding.label().to_string()];
        if let Some(kbps) = self.kbps {
            parts.push(format!("{kbps} kb/s"));
        }
        if let Some(rate) = self.sample_rate {
            parts.push(format!("{} kHz", rate as f64 / 1000.0));
        }
        match self.channels {
            Some(1) => parts.push("mono".into()),
            Some(2) => parts.push("stereo".into()),
            _ => {}
        }
        parts.join(" · ")
    }
}

/// Carry the source's cover art when the output format can hold it.
fn keep_cover(
    encoding: Encoding,
    info: &AudioInfo,
    args: &mut Vec<OsString>,
    warnings: &mut Vec<String>,
) {
    let Some(cover) = &info.cover else { return };
    if encoding.holds_cover() {
        args.extend([
            "-map".into(),
            format!("0:{}", cover.index).into(),
            "-c:v".into(),
            "copy".into(),
            "-disposition:v:0".into(),
            "attached_pic".into(),
        ]);
    } else {
        warnings.push(format!(
            "{} files can't carry cover art, so the cover wasn't kept.",
            encoding.label()
        ));
    }
}

/// Ogg keeps tags on the stream; other containers want them on the file.
fn metadata_args(info: &AudioInfo) -> Vec<OsString> {
    if info.tags_on_stream {
        vec!["-map_metadata".into(), "0:s:a:0".into()]
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Probe
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct AudioStream {
    /// Absolute stream index in the file.
    index: usize,
    codec: String,
    sample_rate: u64,
    channels: u64,
    layout: Option<String>,
    bitrate: Option<u64>,
    bits: Option<u64>,
    language: Option<String>,
}

#[derive(Clone, Debug)]
struct Cover {
    index: usize,
    width: u64,
    height: u64,
}

#[derive(Clone, Debug)]
struct AudioInfo {
    duration: Option<f64>,
    bytes: Option<u64>,
    bitrate: Option<u64>,
    container: String,
    streams: Vec<AudioStream>,
    cover: Option<Cover>,
    tags: BTreeMap<String, String>,
    tags_on_stream: bool,
}

impl AudioInfo {
    fn first(&self) -> Result<&AudioStream, String> {
        self.streams
            .first()
            .ok_or_else(|| "This file has no audio track".to_string())
    }

    fn duration(&self) -> Result<f64, String> {
        self.duration
            .filter(|seconds| *seconds > 0.0)
            .ok_or_else(|| "Could not read the length of this file".to_string())
    }
}

fn number<T: std::str::FromStr>(value: &Value) -> Option<T> {
    match value {
        Value::String(text) => text.parse().ok(),
        Value::Number(number) => number.to_string().parse().ok(),
        _ => None,
    }
}

fn tag_map(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|tags| {
            tags.iter()
                .filter_map(|(key, value)| {
                    Some((key.to_ascii_lowercase(), value.as_str()?.trim().to_string()))
                })
                .filter(|(_, value)| !value.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn probe(ffmpeg: &Path, input: &Path, cancelled: &AtomicBool) -> Result<AudioInfo, String> {
    let value = ffprobe_json(ffmpeg, input, cancelled)?;
    let format = &value["format"];
    let mut streams = Vec::new();
    let mut cover = None;
    let mut stream_tags = BTreeMap::new();
    for stream in value["streams"].as_array().into_iter().flatten() {
        let index = stream["index"].as_u64().unwrap_or(0) as usize;
        match stream["codec_type"].as_str() {
            Some("audio") => {
                if streams.is_empty() {
                    stream_tags = tag_map(&stream["tags"]);
                }
                streams.push(AudioStream {
                    index,
                    codec: stream["codec_name"].as_str().unwrap_or("unknown").into(),
                    sample_rate: number(&stream["sample_rate"]).unwrap_or(48_000),
                    channels: stream["channels"].as_u64().unwrap_or(2),
                    layout: stream["channel_layout"].as_str().map(str::to_owned),
                    bitrate: number(&stream["bit_rate"]),
                    bits: number::<u64>(&stream["bits_per_raw_sample"])
                        .filter(|bits| *bits > 0)
                        .or_else(|| {
                            number::<u64>(&stream["bits_per_sample"]).filter(|bits| *bits > 0)
                        }),
                    language: stream["tags"]["language"]
                        .as_str()
                        .filter(|language| *language != "und")
                        .map(str::to_owned),
                });
            }
            Some("video") if stream["disposition"]["attached_pic"].as_u64() == Some(1) => {
                cover.get_or_insert(Cover {
                    index,
                    width: stream["width"].as_u64().unwrap_or(0),
                    height: stream["height"].as_u64().unwrap_or(0),
                });
            }
            _ => {}
        }
    }
    let mut tags = tag_map(&format["tags"]);
    let tags_on_stream = tags.is_empty() && !stream_tags.is_empty();
    for (key, value) in stream_tags {
        tags.entry(key).or_insert(value);
    }
    // Encoder stamps are not user tags.
    tags.remove("encoder");
    Ok(AudioInfo {
        duration: number(&format["duration"]).filter(|seconds: &f64| seconds.is_finite()),
        bytes: number(&format["size"]),
        bitrate: number(&format["bit_rate"]),
        container: format["format_name"].as_str().unwrap_or("unknown").into(),
        streams,
        cover,
        tags,
        tags_on_stream,
    })
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// Provider capabilities an audio request needs before any work starts.
pub(super) fn required(
    id: &str,
    request: &ToolRequest,
    required: &mut Vec<String>,
) -> Result<(), String> {
    let encoding = |default: &str, required: &mut Vec<String>| -> Result<(), String> {
        if let Some(encoding) = requested_encoding(request, default)? {
            encoding.capabilities(required);
        }
        Ok(())
    };
    match id {
        "arcade.audio.convert" => encoding("mp3", required)?,
        "arcade.audio.trim" => {
            encoding("same", required)?;
            if option_f64(request, "fadeInSeconds", 0.0, 0.0, 30.0)? > 0.0
                || option_f64(request, "fadeOutSeconds", 0.0, 0.0, 30.0)? > 0.0
            {
                required.push("filter:afade".into());
            }
            if option_str(request, "mode", "keep")? == "remove" {
                required.extend([
                    "filter:atrim".into(),
                    "filter:asplit".into(),
                    "filter:acrossfade".into(),
                ]);
            }
        }
        "arcade.audio.join" => {
            encoding("same", required)?;
            if option_f64(request, "crossfadeSeconds", 0.0, 0.0, 10.0)? > 0.0 {
                required.push("filter:acrossfade".into());
            } else {
                required.push("filter:concat".into());
            }
            if option_f64(request, "gapSeconds", 0.0, 0.0, 30.0)? > 0.0 {
                required.push("filter:apad".into());
            }
        }
        "arcade.audio.normalize" => {
            encoding("same", required)?;
            match option_str(request, "mode", "loudness")? {
                "loudness" => required.push("filter:loudnorm".into()),
                "peak" => required.extend(["filter:volumedetect".into(), "filter:volume".into()]),
                other => return Err(format!("Unknown normalization mode: {other}")),
            }
        }
        "arcade.audio.speed-pitch" => {
            encoding("same", required)?;
            required.push("filter:atempo".into());
        }
        "arcade.audio.silence" => {
            encoding("same", required)?;
            required.push("filter:silencedetect".into());
            if option_str(request, "operation", "trim")? == "shorten" {
                required.extend([
                    "filter:atrim".into(),
                    "filter:asplit".into(),
                    "filter:concat".into(),
                ]);
            }
        }
        "arcade.audio.metadata" => {}
        "arcade.audio.denoise" => {
            encoding("same", required)?;
            required.extend(["filter:afftdn".into(), "filter:highpass".into()]);
        }
        "arcade.audio.vocals" => {
            encoding("mp3", required)?;
            required.extend(["filter:stereotools".into(), "filter:amix".into()]);
        }
        _ => return Err(format!("No audio executor is registered for {id}")),
    }
    Ok(())
}

pub(super) fn validate_count(id: &str, request: &ToolRequest, count: usize) -> Result<(), String> {
    if BATCH_TOOLS.contains(&id) {
        return if (1..=MAX_BATCH).contains(&count) {
            Ok(())
        } else {
            Err(format!("Select between 1 and {MAX_BATCH} audio files"))
        };
    }
    match id {
        "arcade.audio.join" if (2..=super::MAX_INPUTS).contains(&count) => Ok(()),
        "arcade.audio.join" => Err("Select at least two clips to join".into()),
        "arcade.audio.metadata" => match option_str(request, "operation", "inspect")? {
            "inspect" if count == 1 => Ok(()),
            "inspect" => Err("Tag inspection takes one audio file".into()),
            "edit" if (1..=2).contains(&count) => Ok(()),
            "edit" => Err("Tag editing takes an audio file and at most one cover image".into()),
            other => Err(format!("Unknown metadata operation: {other}")),
        },
        _ if count == 1 => Ok(()),
        _ => Err("Choose one audio file".into()),
    }
}

pub(super) fn execute(
    manifest: &ToolManifest,
    provider: &crate::provider::ProviderInfo,
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
    let input = paths.first().ok_or("Choose an audio file first")?;
    match job.manifest.id.as_str() {
        "arcade.audio.convert" => convert(job, input),
        "arcade.audio.trim" => trim(job, input),
        "arcade.audio.join" => join(job, paths),
        "arcade.audio.normalize" => normalize(job, input),
        "arcade.audio.speed-pitch" => speed_pitch(job, input),
        "arcade.audio.silence" => silence(job, input),
        "arcade.audio.metadata" => metadata(job, paths),
        "arcade.audio.denoise" => enhance::denoise(job, input),
        "arcade.audio.vocals" => enhance::vocals(job, input),
        other => Err(format!("No audio executor is registered for {other}")),
    }
}

/// Fail early, with a readable reason, when a "same as source" encoder is missing.
fn ensure_encoder(job: &Job, encoding: Encoding) -> Result<(), String> {
    let mut needed = Vec::new();
    encoding.capabilities(&mut needed);
    if needed
        .iter()
        .all(|capability| job.provider.capabilities.contains(capability))
    {
        Ok(())
    } else {
        Err(format!(
            "The installed FFmpeg can't write {} ({} missing). Choose another format.",
            encoding.label(),
            needed.join(", ")
        ))
    }
}

fn has_filter(job: &Job, name: &str) -> bool {
    job.provider
        .capabilities
        .iter()
        .any(|capability| capability == &format!("filter:{name}"))
}

/// A scaled view of the job's progress, for multi-pass work.
fn phase(progress: &Progress, from: f64, span: f64) -> Progress {
    let progress = progress.clone();
    Arc::new(move |fraction| progress(from + fraction * span))
}

fn fade_filters(fade_in: f64, fade_out: f64, length: Option<f64>) -> Result<Vec<String>, String> {
    let mut filters = Vec::new();
    if fade_in > 0.0 {
        filters.push(format!("afade=t=in:st=0:d={}", format_seconds(fade_in)));
    }
    if fade_out > 0.0 {
        let length = length.ok_or("The fade-out needs the file length, which couldn't be read")?;
        filters.push(format!(
            "afade=t=out:st={}:d={}",
            format_seconds((length - fade_out).max(0.0)),
            format_seconds(fade_out)
        ));
    }
    if let Some(length) = length
        && fade_in + fade_out > length + 0.001
    {
        return Err(format!(
            "The fades ({} s) are longer than the audio that's kept ({} s)",
            format_seconds(fade_in + fade_out),
            format_seconds((length * 100.0).round() / 100.0)
        ));
    }
    Ok(filters)
}

// ---------------------------------------------------------------------------
// Convert
// ---------------------------------------------------------------------------

fn convert(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    let stream = info.first()?.clone();
    let (output, mut warnings) = Output::resolve(job.request, "mp3", &stream)?;
    let mut args: Vec<OsString> = vec!["-map".into(), format!("0:{}", stream.index).into()];
    keep_cover(output.encoding, &info, &mut args, &mut warnings);
    args.extend(metadata_args(&info));
    args.extend(output.codec_args(&stream, false));
    let stage = stage_directory(input)?;
    let mut result = produce(
        job,
        input,
        stage.path(),
        &[MediaInput::plain(input)],
        args,
        output.encoding.extension(),
        "converted",
        info.duration,
        None,
    )?;
    result.message = Some(format!(
        "Saved {} ({})",
        output_name(&result),
        output.describe()
    ));
    result.warnings = warnings;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Trim / cut
// ---------------------------------------------------------------------------

fn trim(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let request = job.request;
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    let stream = info.first()?.clone();
    let (start, end) = validate_range(request)?;
    let start = start.unwrap_or(0.0);
    if let Some(total) = info.duration
        && start >= total
    {
        return Err(format!(
            "The start time is after the end of the file ({}).",
            clock(total)
        ));
    }
    let end = end.map(|end| info.duration.map_or(end, |total| end.min(total)));
    let fade_in = option_f64(request, "fadeInSeconds", 0.0, 0.0, 30.0)?;
    let fade_out = option_f64(request, "fadeOutSeconds", 0.0, 0.0, 30.0)?;
    let precise = option_bool(request, "precise", false)?;
    let stage = stage_directory(input)?;
    match option_str(request, "mode", "keep")? {
        "keep" => {
            if start <= 0.0 && end.is_none() && fade_in == 0.0 && fade_out == 0.0 {
                return Err("Choose a start or end time to trim".into());
            }
            let length = end.or(info.duration).map(|end| end - start);
            let mut source = MediaInput::plain(input);
            if start > 0.0 {
                source.options = vec!["-ss".into(), format_seconds(start).into()];
            }
            let mut args: Vec<OsString> = vec!["-map".into(), format!("0:{}", stream.index).into()];
            if let Some(end) = end {
                args.extend(["-t".into(), format_seconds(end - start).into()]);
            }
            let copy = option_str(request, "format", "same")? == "same"
                && !precise
                && fade_in == 0.0
                && fade_out == 0.0;
            let mut warnings = Vec::new();
            let (extension, method, described) = match copy_target(&stream.codec).filter(|_| copy) {
                Some((extension, muxer)) => {
                    args.extend(["-c:a".into(), "copy".into()]);
                    if let Some(encoding) = Encoding::of_codec(&stream.codec) {
                        keep_cover(encoding, &info, &mut args, &mut warnings);
                    }
                    args.extend(metadata_args(&info));
                    args.extend(["-f".into(), muxer.into()]);
                    (extension, "copy", "original quality".to_string())
                }
                None => {
                    let (output, notes) = Output::resolve(request, "same", &stream)?;
                    ensure_encoder(job, output.encoding)?;
                    warnings.extend(notes);
                    let filters = fade_filters(fade_in, fade_out, length)?;
                    if !filters.is_empty() {
                        args.extend(["-af".into(), filters.join(",").into()]);
                    }
                    keep_cover(output.encoding, &info, &mut args, &mut warnings);
                    args.extend(metadata_args(&info));
                    args.extend(output.codec_args(&stream, false));
                    (output.encoding.extension(), "reencode", output.describe())
                }
            };
            let mut result = produce(
                job,
                input,
                stage.path(),
                &[source],
                args,
                extension,
                "trimmed",
                length,
                None,
            )?;
            let span = match length {
                Some(length) => format!(
                    "{}–{}, {} s",
                    clock(start),
                    clock(start + length),
                    format_seconds((length * 100.0).round() / 100.0)
                ),
                None => format!("from {}", clock(start)),
            };
            result.message = Some(format!(
                "Saved {} ({span}, {described})",
                output_name(&result)
            ));
            result.metadata.insert("method".into(), json!(method));
            result.warnings = warnings;
            Ok(result)
        }
        "remove" => {
            let total = info.duration()?;
            let end = end.unwrap_or(total);
            if start <= 0.001 && end >= total - 0.001 {
                return Err("That would remove the whole file".into());
            }
            let (output, mut warnings) = Output::resolve(request, "same", &stream)?;
            ensure_encoder(job, output.encoding)?;
            let kept_before = start;
            let kept_after = total - end;
            let source = format!("0:{}", stream.index);
            let (graph, kept) = if kept_before <= 0.001 {
                (
                    format!(
                        "[{source}]atrim=start={},asetpts=PTS-STARTPTS",
                        format_seconds(end)
                    ),
                    kept_after,
                )
            } else if kept_after <= 0.001 {
                (
                    format!("[{source}]atrim=end={}", format_seconds(start)),
                    kept_before,
                )
            } else if kept_before >= 0.05 && kept_after >= 0.05 {
                // A 10 ms crossfade hides the click a hard splice can make.
                (
                    format!(
                        "[{source}]asplit=2[a][b];[a]atrim=end={s}[x];[b]atrim=start={e},asetpts=PTS-STARTPTS[y];[x][y]acrossfade=d=0.01:c1=tri:c2=tri",
                        s = format_seconds(start),
                        e = format_seconds(end)
                    ),
                    kept_before + kept_after - 0.01,
                )
            } else {
                (
                    format!(
                        "[{source}]asplit=2[a][b];[a]atrim=end={s}[x];[b]atrim=start={e},asetpts=PTS-STARTPTS[y];[x][y]concat=n=2:v=0:a=1",
                        s = format_seconds(start),
                        e = format_seconds(end)
                    ),
                    kept_before + kept_after,
                )
            };
            let mut graph = graph;
            for filter in fade_filters(fade_in, fade_out, Some(kept))? {
                graph.push(',');
                graph.push_str(&filter);
            }
            graph.push_str("[out]");
            let mut args: Vec<OsString> = vec![
                "-filter_complex".into(),
                graph.into(),
                "-map".into(),
                "[out]".into(),
            ];
            keep_cover(output.encoding, &info, &mut args, &mut warnings);
            args.extend(metadata_args(&info));
            args.extend(output.codec_args(&stream, false));
            let mut result = produce(
                job,
                input,
                stage.path(),
                &[MediaInput::plain(input)],
                args,
                output.encoding.extension(),
                "cut",
                Some(kept),
                None,
            )?;
            result.message = Some(format!(
                "Removed {}–{} ({} s) and saved {}",
                clock(start),
                clock(end),
                format_seconds(((end - start) * 100.0).round() / 100.0),
                output_name(&result)
            ));
            result.metadata.insert("method".into(), json!("reencode"));
            result.warnings = warnings;
            Ok(result)
        }
        other => Err(format!("Unknown trim mode: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Join
// ---------------------------------------------------------------------------

fn copy_mismatch(streams: &[&AudioStream]) -> Option<String> {
    let first = streams.first()?;
    if copy_target(&first.codec).is_none() {
        return Some(format!(
            "{} audio can't be joined without re-encoding",
            first.codec
        ));
    }
    for (index, stream) in streams.iter().enumerate().skip(1) {
        let clip = index + 1;
        if stream.codec != first.codec {
            return Some(format!(
                "clip {clip} is {} and clip 1 is {}",
                stream.codec, first.codec
            ));
        }
        if stream.sample_rate != first.sample_rate {
            return Some(format!(
                "clip {clip} is {} kHz and clip 1 is {} kHz",
                stream.sample_rate as f64 / 1000.0,
                first.sample_rate as f64 / 1000.0
            ));
        }
        if stream.channels != first.channels {
            return Some(format!(
                "clip {clip} has {} channels and clip 1 has {}",
                stream.channels, first.channels
            ));
        }
    }
    None
}

fn join(job: &Job, inputs: &[PathBuf]) -> Result<ToolResult, String> {
    let request = job.request;
    let ffmpeg = &job.provider.executable_path;
    let mut infos = Vec::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        let info = probe(ffmpeg, input, job.cancelled)?;
        if info.streams.is_empty() {
            return Err(format!("Clip {} has no audio", index + 1));
        }
        infos.push(info);
    }
    let streams: Vec<&AudioStream> = infos.iter().map(|info| &info.streams[0]).collect();
    let crossfade = option_f64(request, "crossfadeSeconds", 0.0, 0.0, 10.0)?;
    let gap = option_f64(request, "gapSeconds", 0.0, 0.0, 30.0)?;
    if crossfade > 0.0 && gap > 0.0 {
        return Err("Use either a crossfade or a gap between clips, not both".into());
    }
    let joins = (inputs.len() - 1) as f64;
    let total = infos
        .iter()
        .map(|info| info.duration)
        .sum::<Option<f64>>()
        .map(|sum| sum + gap * joins - crossfade * joins);
    let same = option_str(request, "format", "same")? == "same";
    let mismatch = copy_mismatch(&streams);
    let stage = stage_directory(&inputs[0])?;
    if same && crossfade == 0.0 && gap == 0.0 && mismatch.is_none() {
        let (extension, muxer) = copy_target(&streams[0].codec).expect("checked by copy_mismatch");
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
        fs::write(&list, script)
            .map_err(|error| format!("Cannot prepare the clip list: {error}"))?;
        let mut result = produce(
            job,
            &inputs[0],
            stage.path(),
            &[MediaInput {
                options: vec!["-f".into(), "concat".into(), "-safe".into(), "0".into()],
                path: list,
            }],
            vec![
                "-map".into(),
                "0:a:0".into(),
                "-c:a".into(),
                "copy".into(),
                "-f".into(),
                muxer.into(),
            ],
            extension,
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
        return Ok(result);
    }

    let (output, mut warnings) = Output::resolve(request, "same", streams[0])?;
    ensure_encoder(job, output.encoding)?;
    if same
        && crossfade == 0.0
        && gap == 0.0
        && let Some(reason) = &mismatch
    {
        warnings.push(format!("Re-encoded because {reason}."));
    }
    if crossfade > 0.0 {
        for (index, info) in infos.iter().enumerate() {
            if info.duration.is_some_and(|length| length <= crossfade) {
                return Err(format!(
                    "Clip {} is shorter than the {} s crossfade",
                    index + 1,
                    format_seconds(crossfade)
                ));
            }
        }
    }
    let rate = output
        .encoding
        .fit_rate(output.sample_rate.unwrap_or(streams[0].sample_rate));
    let most = streams
        .iter()
        .map(|stream| stream.channels)
        .max()
        .unwrap_or(2);
    let layout = match most {
        1 => "mono".to_string(),
        2 => "stereo".to_string(),
        _ => streams
            .iter()
            .find(|stream| stream.channels == most)
            .and_then(|stream| stream.layout.clone())
            .unwrap_or_else(|| "5.1".into()),
    };
    let mut graph = String::new();
    for (index, stream) in streams.iter().enumerate() {
        let pad = if gap > 0.0 && index + 1 < streams.len() {
            format!(",apad=pad_dur={}", format_seconds(gap))
        } else {
            String::new()
        };
        graph.push_str(&format!(
            "[{index}:{}]aresample={rate},aformat=sample_fmts=fltp:sample_rates={rate}:channel_layouts={layout}{pad}[a{index}];",
            stream.index
        ));
    }
    if crossfade > 0.0 {
        let mut previous = "a0".to_string();
        for index in 1..streams.len() {
            let label = if index + 1 == streams.len() {
                "out".to_string()
            } else {
                format!("x{index}")
            };
            graph.push_str(&format!(
                "[{previous}][a{index}]acrossfade=d={}:c1=tri:c2=tri[{label}];",
                format_seconds(crossfade)
            ));
            previous = label;
        }
        graph.pop();
    } else {
        for index in 0..streams.len() {
            graph.push_str(&format!("[a{index}]"));
        }
        graph.push_str(&format!("concat=n={}:v=0:a=1[out]", streams.len()));
    }
    let mut args: Vec<OsString> = vec![
        "-filter_complex".into(),
        graph.into(),
        "-map".into(),
        "[out]".into(),
    ];
    args.extend(metadata_args(&infos[0]));
    args.extend(output.codec_args(streams[0], false));
    let sources: Vec<MediaInput> = inputs.iter().map(|path| MediaInput::plain(path)).collect();
    let mut result = produce(
        job,
        &inputs[0],
        stage.path(),
        &sources,
        args,
        output.encoding.extension(),
        "joined",
        total,
        None,
    )?;
    let joined_how = if crossfade > 0.0 {
        format!(" with {} s crossfades", format_seconds(crossfade))
    } else if gap > 0.0 {
        format!(" with {} s gaps", format_seconds(gap))
    } else {
        String::new()
    };
    result.message = Some(format!(
        "Joined {} clips{joined_how} into {} ({})",
        inputs.len(),
        output_name(&result),
        output.describe()
    ));
    result.metadata.insert("method".into(), json!("reencode"));
    result.warnings = warnings;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Normalize
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoudnessReport {
    /// Integrated loudness, LUFS.
    pub integrated: f64,
    /// True peak, dBTP.
    pub true_peak: f64,
    /// Loudness range, LU.
    pub range: f64,
    pub threshold: f64,
}

fn loudnorm_json(stderr: &str) -> Option<Value> {
    let end = stderr.rfind('}')? + 1;
    let start = stderr[..end].rfind('{')?;
    serde_json::from_str(&stderr[start..end]).ok()
}

fn loudnorm_number(value: &Value, key: &str) -> f64 {
    value[key]
        .as_str()
        .and_then(|text| text.trim().parse::<f64>().ok())
        .unwrap_or(f64::NEG_INFINITY)
}

/// Run an analysis-only pass and return FFmpeg's log.
fn analyse(
    job: &Job,
    input: &Path,
    stream: usize,
    filter: &str,
    duration: Option<f64>,
    progress: Progress,
) -> Result<String, String> {
    let output = run_ffmpeg(
        &job.provider.executable_path,
        &[MediaInput::plain(input)],
        vec![
            "-map".into(),
            format!("0:{stream}").into(),
            "-af".into(),
            filter.into(),
            "-f".into(),
            "null".into(),
            "-".into(),
        ],
        &std::env::temp_dir(),
        duration,
        job.cancelled,
        progress,
        "info",
    )?;
    Ok(String::from_utf8_lossy(&output.stderr).into_owned())
}

fn measure_loudness(
    job: &Job,
    input: &Path,
    stream: usize,
    duration: Option<f64>,
    progress: Progress,
) -> Result<(LoudnessReport, Value), String> {
    let log = analyse(
        job,
        input,
        stream,
        "loudnorm=print_format=json",
        duration,
        progress,
    )?;
    let stats = loudnorm_json(&log).ok_or("FFmpeg did not report loudness measurements")?;
    let report = LoudnessReport {
        integrated: loudnorm_number(&stats, "input_i"),
        true_peak: loudnorm_number(&stats, "input_tp"),
        range: loudnorm_number(&stats, "input_lra"),
        threshold: loudnorm_number(&stats, "input_thresh"),
    };
    Ok((report, stats))
}

fn finite(value: f64) -> Value {
    if value.is_finite() {
        json!((value * 100.0).round() / 100.0)
    } else {
        Value::Null
    }
}

fn normalize(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let request = job.request;
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    let stream = info.first()?.clone();
    let (output, mut warnings) = Output::resolve(request, "same", &stream)?;
    ensure_encoder(job, output.encoding)?;
    let mode = option_str(request, "mode", "loudness")?;
    let (filter, report) = match mode {
        "loudness" => {
            let target = match option_str(request, "preset", "podcast")? {
                "streaming" | "music" => -14.0,
                "podcast" => -16.0,
                "speech" => -18.0,
                "broadcast" => -23.0,
                "custom" => option_f64(request, "targetLufs", -16.0, -36.0, -6.0)?,
                other => return Err(format!("Unknown loudness preset: {other}")),
            };
            let peak = option_f64(request, "truePeakDb", -1.0, -9.0, 0.0)?;
            let range = option_f64(request, "loudnessRange", 11.0, 1.0, 20.0)?;
            let (before, stats) = measure_loudness(
                job,
                input,
                stream.index,
                info.duration,
                phase(&job.progress, 0.0, 0.5),
            )?;
            if !before.integrated.is_finite() || before.integrated < -70.0 {
                return Err("This file is silent, so there's nothing to normalize".into());
            }
            let filter = format!(
                "loudnorm=I={target}:TP={peak}:LRA={range}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true:print_format=json",
                before.integrated,
                before.true_peak,
                before.range,
                before.threshold,
                loudnorm_number(&stats, "target_offset").max(-99.0),
            );
            (
                filter,
                json!({
                    "mode": "loudness",
                    "target": { "integrated": target, "truePeak": peak, "range": range },
                    "before": { "integrated": finite(before.integrated), "truePeak": finite(before.true_peak), "range": finite(before.range) },
                }),
            )
        }
        "peak" => {
            let target = option_f64(request, "peakDb", -1.0, -20.0, 0.0)?;
            let log = analyse(
                job,
                input,
                stream.index,
                "volumedetect",
                info.duration,
                phase(&job.progress, 0.0, 0.5),
            )?;
            let peak = log
                .lines()
                .find_map(|line| {
                    let value = line.split("max_volume:").nth(1)?;
                    value
                        .trim()
                        .trim_end_matches("dB")
                        .trim()
                        .parse::<f64>()
                        .ok()
                })
                .ok_or("FFmpeg did not report the peak level")?;
            if peak <= -90.0 {
                return Err("This file is silent, so there's nothing to normalize".into());
            }
            if output.encoding.lossy() && target > -1.0 {
                warnings.push(
                    "Lossy encoders can push peaks slightly above the target; -1 dB leaves room."
                        .into(),
                );
            }
            let gain = target - peak;
            (
                format!("volume={gain:.2}dB"),
                json!({
                    "mode": "peak",
                    "target": { "peak": target },
                    "before": { "peak": peak },
                    "gainDb": (gain * 100.0).round() / 100.0,
                }),
            )
        }
        other => return Err(format!("Unknown normalization mode: {other}")),
    };
    let mut args: Vec<OsString> = vec![
        "-map".into(),
        format!("0:{}", stream.index).into(),
        "-af".into(),
        filter.into(),
    ];
    keep_cover(output.encoding, &info, &mut args, &mut warnings);
    args.extend(metadata_args(&info));
    args.extend(output.codec_args(&stream, mode == "loudness"));
    let stage = stage_directory(input)?;
    let staged = stage
        .path()
        .join(format!("output.{}", output.encoding.extension()));
    args.extend(["-n".into(), staged.as_os_str().to_os_string()]);
    let log = run_ffmpeg(
        &job.provider.executable_path,
        &[MediaInput::plain(input)],
        args,
        stage.path(),
        info.duration,
        job.cancelled,
        phase(&job.progress, 0.5, 0.5),
        "info",
    )?;
    let name = requested_output_name(request, input, "normalized", output.encoding.extension())?;
    let mut result = publish_named(job, input, &staged, &name, None)?;
    let mut report = report;
    let message = if mode == "loudness" {
        let stats = loudnorm_json(&String::from_utf8_lossy(&log.stderr)).unwrap_or(Value::Null);
        let after = loudnorm_number(&stats, "output_i");
        let linear = stats["normalization_type"].as_str() != Some("dynamic");
        if !linear {
            warnings.push("Reaching this loudness within the peak limit needed dynamic processing, which slightly changes the dynamics. Lower the target or allow a higher true peak for a plain gain change.".into());
        }
        report["after"] = json!({
            "integrated": finite(after),
            "truePeak": finite(loudnorm_number(&stats, "output_tp")),
            "range": finite(loudnorm_number(&stats, "output_lra")),
        });
        report["method"] = json!(if linear { "linear" } else { "dynamic" });
        let before = report["before"]["integrated"].as_f64().unwrap_or(f64::NAN);
        format!(
            "Loudness {before:.1} → {} LUFS · saved {}",
            if after.is_finite() {
                format!("{after:.1}")
            } else {
                "?".into()
            },
            output_name(&result)
        )
    } else {
        let gain = report["gainDb"].as_f64().unwrap_or(0.0);
        report["after"] = json!({ "peak": report["target"]["peak"] });
        format!("Peak gain {gain:+.1} dB · saved {}", output_name(&result))
    };
    result.outputs.push(ToolValue::text(
        serde_json::to_string_pretty(&report).unwrap_or_default(),
        "structured/audio-loudness",
    ));
    result.message = Some(message);
    result.metadata.insert("loudness".into(), report);
    result.warnings = warnings;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Speed / pitch
// ---------------------------------------------------------------------------

/// atempo accepts 0.5–2.0 per instance on every supported FFmpeg.
fn atempo_chain(mut factor: f64) -> Vec<String> {
    let mut parts = Vec::new();
    while factor < 0.5 {
        parts.push("atempo=0.5".to_string());
        factor /= 0.5;
    }
    while factor > 2.0 {
        parts.push("atempo=2.0".to_string());
        factor /= 2.0;
    }
    if (factor - 1.0).abs() > 1e-6 {
        parts.push(format!("atempo={factor:.6}"));
    }
    parts
}

fn speed_pitch(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let request = job.request;
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    let stream = info.first()?.clone();
    let speed = option_f64(request, "speed", 1.0, 0.25, 4.0)?;
    let preserve = option_bool(request, "preservePitch", true)?;
    let semitones = if preserve {
        option_f64(request, "pitchSemitones", 0.0, -12.0, 12.0)?
    } else {
        0.0
    };
    if (speed - 1.0).abs() < 1e-6 && semitones == 0.0 {
        return Err("Choose a speed or pitch change".into());
    }
    let (output, mut warnings) = Output::resolve(request, "same", &stream)?;
    ensure_encoder(job, output.encoding)?;
    let rate = stream.sample_rate;
    let ratio = 2_f64.powf(semitones / 12.0);
    let (filter, engine) = if !preserve {
        (
            format!("asetrate={:.0},aresample={rate}", rate as f64 * speed),
            "resample",
        )
    } else if semitones == 0.0 {
        (atempo_chain(speed).join(","), "atempo")
    } else if has_filter(job, "rubberband") {
        (
            format!("rubberband=tempo={speed:.6}:pitch={ratio:.6}:pitchq=quality"),
            "rubberband",
        )
    } else {
        let mut parts = vec![
            format!("asetrate={:.0}", rate as f64 * ratio),
            format!("aresample={rate}"),
        ];
        parts.extend(atempo_chain(speed / ratio));
        (parts.join(","), "resample+atempo")
    };
    let length = info.duration.map(|seconds| seconds / speed);
    let mut args: Vec<OsString> = vec![
        "-map".into(),
        format!("0:{}", stream.index).into(),
        "-af".into(),
        filter.into(),
    ];
    keep_cover(output.encoding, &info, &mut args, &mut warnings);
    args.extend(metadata_args(&info));
    args.extend(output.codec_args(&stream, false));
    let stage = stage_directory(input)?;
    let mut result = produce(
        job,
        input,
        stage.path(),
        &[MediaInput::plain(input)],
        args,
        output.encoding.extension(),
        "speed",
        length,
        None,
    )?;
    let mut change = vec![format!("{}× speed", format_seconds(speed))];
    if semitones != 0.0 {
        change.push(format!(
            "pitch {:+} semitone{}",
            semitones,
            if semitones.abs() == 1.0 { "" } else { "s" }
        ));
    } else if !preserve && speed != 1.0 {
        change.push("pitch follows speed".into());
    }
    if let Some(length) = length {
        change.push(format!("new length {}", clock(length)));
    }
    result.message = Some(format!(
        "Saved {} ({})",
        output_name(&result),
        change.join(", ")
    ));
    result.metadata.insert("engine".into(), json!(engine));
    result.warnings = warnings;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Silence
// ---------------------------------------------------------------------------

/// Silent stretches `(start, end)` from a silencedetect log.
fn silences(log: &str, duration: f64) -> Vec<(f64, f64)> {
    let mut spans = Vec::new();
    let mut open: Option<f64> = None;
    for line in log.lines() {
        if let Some(value) = line.split("silence_start:").nth(1) {
            open = first_number(value);
        } else if let Some(value) = line.split("silence_end:").nth(1)
            && let Some(end) = first_number(value)
        {
            let start = open.take().unwrap_or(0.0);
            spans.push((start.clamp(0.0, duration), end.clamp(0.0, duration)));
        }
    }
    if let Some(start) = open {
        spans.push((start.clamp(0.0, duration), duration));
    }
    spans.retain(|(start, end)| end > start);
    spans
}

fn first_number(text: &str) -> Option<f64> {
    text.split(|ch: char| ch.is_whitespace() || ch == '|')
        .find(|piece| !piece.is_empty())?
        .parse()
        .ok()
}

/// Sound between the silent stretches, widened by `pad` on each side.
fn audible(silences: &[(f64, f64)], duration: f64, pad: f64) -> Vec<(f64, f64)> {
    let mut spans = Vec::new();
    let mut cursor = 0.0;
    for (start, end) in silences {
        if *start > cursor {
            spans.push((cursor, *start));
        }
        cursor = cursor.max(*end);
    }
    if duration > cursor {
        spans.push((cursor, duration));
    }
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (start, end) in spans {
        let (start, end) = ((start - pad).max(0.0), (end + pad).min(duration));
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

fn detect_silence(
    job: &Job,
    input: &Path,
    stream: usize,
    threshold: f64,
    minimum: f64,
    duration: f64,
) -> Result<Vec<(f64, f64)>, String> {
    let log = analyse(
        job,
        input,
        stream,
        &format!("silencedetect=noise={threshold:.2}dB:d={minimum:.3}"),
        Some(duration),
        phase(&job.progress, 0.0, 0.4),
    )?;
    Ok(silences(&log, duration))
}

fn silence(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let request = job.request;
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    let stream = info.first()?.clone();
    let duration = info.duration()?;
    let threshold = option_f64(request, "thresholdDb", -45.0, -90.0, -10.0)?;
    let minimum = option_f64(request, "minimumSeconds", 0.5, 0.05, 30.0)?;
    let operation = option_str(request, "operation", "trim")?;
    let (output, mut warnings) = Output::resolve(request, "same", &stream)?;
    ensure_encoder(job, output.encoding)?;
    let quiet = detect_silence(job, input, stream.index, threshold, minimum, duration)?;
    let silent_total: f64 = quiet.iter().map(|(start, end)| end - start).sum();
    if quiet.len() == 1 && quiet[0].0 <= 0.01 && quiet[0].1 >= duration - 0.01 {
        return Err(format!(
            "The whole file is quieter than {threshold} dB. Lower the threshold to find the sound."
        ));
    }
    let stage = stage_directory(input)?;
    let rest = phase(&job.progress, 0.4, 0.6);
    let summary = json!({
        "silences": quiet.iter().map(|(start, end)| json!([start, end])).collect::<Vec<_>>(),
        "silentSeconds": (silent_total * 100.0).round() / 100.0,
    });
    match operation {
        "trim" => {
            let pad = option_f64(request, "paddingSeconds", 0.1, 0.0, 5.0)?;
            let lead = quiet
                .first()
                .filter(|(start, _)| *start <= 0.01)
                .map(|(_, end)| *end);
            let trail = quiet
                .last()
                .filter(|(_, end)| *end >= duration - 0.01)
                .map(|(start, _)| *start);
            if lead.is_none() && trail.is_none() {
                return Err(format!(
                    "No silence longer than {} s at the start or end",
                    format_seconds(minimum)
                ));
            }
            let start = lead.map_or(0.0, |end| (end - pad).max(0.0));
            let end = trail.map_or(duration, |start| (start + pad).min(duration));
            let mut args: Vec<OsString> = vec![
                "-map".into(),
                format!("0:{}", stream.index).into(),
                "-t".into(),
                format_seconds(end - start).into(),
            ];
            keep_cover(output.encoding, &info, &mut args, &mut warnings);
            args.extend(metadata_args(&info));
            args.extend(output.codec_args(&stream, false));
            let item = Job {
                progress: rest,
                ..*job
            };
            let mut result = produce(
                &item,
                input,
                stage.path(),
                &[MediaInput {
                    options: vec!["-ss".into(), format_seconds(start).into()],
                    path: input.to_path_buf(),
                }],
                args,
                output.encoding.extension(),
                "trimmed",
                Some(end - start),
                None,
            )?;
            result.message = Some(format!(
                "Removed {} s at the start and {} s at the end · saved {}",
                format_seconds((start * 10.0).round() / 10.0),
                format_seconds(((duration - end) * 10.0).round() / 10.0),
                output_name(&result)
            ));
            result.metadata.insert("silence".into(), summary);
            result.warnings = warnings;
            Ok(result)
        }
        "shorten" => {
            let keep = option_f64(request, "keepSeconds", 0.3, 0.0, 5.0)?;
            if quiet.is_empty() {
                return Err(format!(
                    "No pauses longer than {} s were found",
                    format_seconds(minimum)
                ));
            }
            let ranges = audible(&quiet, duration, keep / 2.0);
            if ranges.len() > MAX_SEGMENTS {
                return Err(format!(
                    "This file has {} pauses; the limit is {MAX_SEGMENTS}. Raise the minimum silence length.",
                    ranges.len()
                ));
            }
            let kept: f64 = ranges.iter().map(|(start, end)| end - start).sum();
            let source = format!("0:{}", stream.index);
            let mut graph = String::new();
            if ranges.len() == 1 {
                let (start, end) = ranges[0];
                graph.push_str(&format!(
                    "[{source}]atrim=start={}:end={},asetpts=PTS-STARTPTS[out]",
                    format_seconds(start),
                    format_seconds(end)
                ));
            } else {
                graph.push_str(&format!("[{source}]asplit={}", ranges.len()));
                for index in 0..ranges.len() {
                    graph.push_str(&format!("[s{index}]"));
                }
                graph.push(';');
                for (index, (start, end)) in ranges.iter().enumerate() {
                    graph.push_str(&format!(
                        "[s{index}]atrim=start={}:end={},asetpts=PTS-STARTPTS[p{index}];",
                        format_seconds(*start),
                        format_seconds(*end)
                    ));
                }
                for index in 0..ranges.len() {
                    graph.push_str(&format!("[p{index}]"));
                }
                graph.push_str(&format!("concat=n={}:v=0:a=1[out]", ranges.len()));
            }
            let mut args: Vec<OsString> = vec![
                "-filter_complex".into(),
                graph.into(),
                "-map".into(),
                "[out]".into(),
            ];
            keep_cover(output.encoding, &info, &mut args, &mut warnings);
            args.extend(metadata_args(&info));
            args.extend(output.codec_args(&stream, false));
            let item = Job {
                progress: rest,
                ..*job
            };
            let mut result = produce(
                &item,
                input,
                stage.path(),
                &[MediaInput::plain(input)],
                args,
                output.encoding.extension(),
                "tightened",
                Some(kept),
                None,
            )?;
            result.message = Some(format!(
                "Shortened {} pause{} and removed {} s (new length {}) · saved {}",
                quiet.len(),
                if quiet.len() == 1 { "" } else { "s" },
                format_seconds(((duration - kept) * 10.0).round() / 10.0),
                clock(kept),
                output_name(&result)
            ));
            result.metadata.insert("silence".into(), summary);
            result.warnings = warnings;
            Ok(result)
        }
        "split" => {
            let pad = option_f64(request, "paddingSeconds", 0.1, 0.0, 5.0)?;
            let segments: Vec<(f64, f64)> = audible(&quiet, duration, pad)
                .into_iter()
                .filter(|(start, end)| end - start >= minimum.min(0.25))
                .collect();
            if segments.len() < 2 {
                return Err(format!(
                    "No silence longer than {} s separates parts of this file, so there's nothing to split",
                    format_seconds(minimum)
                ));
            }
            if segments.len() > MAX_SEGMENTS {
                return Err(format!(
                    "This would make {} files; the limit is {MAX_SEGMENTS}. Raise the minimum silence length.",
                    segments.len()
                ));
            }
            split(
                job, input, &info, &stream, &output, &segments, warnings, summary, rest, &stage,
            )
        }
        other => Err(format!("Unknown silence operation: {other}")),
    }
}

#[allow(clippy::too_many_arguments)]
fn split(
    job: &Job,
    input: &Path,
    info: &AudioInfo,
    stream: &AudioStream,
    output: &Output,
    segments: &[(f64, f64)],
    mut warnings: Vec<String>,
    summary: Value,
    progress: Progress,
    stage: &tempfile::TempDir,
) -> Result<ToolResult, String> {
    let base = match job
        .request
        .options
        .get("outputName")
        .and_then(Value::as_str)
    {
        Some(value) if !value.trim().is_empty() => {
            let path = Path::new(value.trim());
            sanitize_piece(&path.file_stem().unwrap_or_default().to_string_lossy())
        }
        _ => sanitize_piece(&input.file_stem().unwrap_or_default().to_string_lossy()),
    };
    let base = if base.is_empty() {
        "part".to_string()
    } else {
        base
    };
    let total: f64 = segments.iter().map(|(start, end)| end - start).sum();
    let mut done = 0.0;
    let mut outputs = Vec::new();
    let mut names = Vec::new();
    let extension = output.encoding.extension();
    for (index, &(start, end)) in segments.iter().enumerate() {
        if job.cancelled.load(Ordering::Relaxed) {
            return Err(format!(
                "Cancelled after {} of {} parts; finished parts were kept",
                names.len(),
                segments.len()
            ));
        }
        let staged = stage
            .path()
            .join(format!("part-{:04}.{extension}", index + 1));
        let mut args: Vec<OsString> = vec![
            "-map".into(),
            format!("0:{}", stream.index).into(),
            "-t".into(),
            format_seconds(end - start).into(),
        ];
        let mut ignored = Vec::new();
        keep_cover(output.encoding, info, &mut args, &mut ignored);
        args.extend(metadata_args(info));
        args.extend(output.codec_args(stream, false));
        args.extend(["-n".into(), staged.as_os_str().to_os_string()]);
        let offset = done;
        let progress = progress.clone();
        run_ffmpeg(
            &job.provider.executable_path,
            &[MediaInput {
                options: vec!["-ss".into(), format_seconds(start).into()],
                path: input.to_path_buf(),
            }],
            args,
            stage.path(),
            Some(end - start),
            job.cancelled,
            Arc::new(move |fraction| progress((offset + fraction * (end - start)) / total)),
            "error",
        )?;
        done += end - start;
        let name = format!("{base}-{:02}.{extension}", index + 1);
        validate_portable_filename(&name)?;
        let saved = publish_named(job, input, &staged, &name, None).map_err(|error| {
            format!(
                "Saved {} of {} parts, then: {error}",
                names.len(),
                segments.len()
            )
        })?;
        names.push(
            saved
                .metadata
                .get("outputName")
                .cloned()
                .unwrap_or(json!(name)),
        );
        outputs.extend(saved.outputs);
        if index == 0 {
            warnings.extend(ignored);
        }
    }
    let segment_list = json!(
        segments
            .iter()
            .map(|(start, end)| json!({ "startSeconds": start, "endSeconds": end }))
            .collect::<Vec<_>>()
    );
    let mut result = base_result(
        job.manifest,
        outputs,
        &format!("Split into {} parts ({})", names.len(), output.describe()),
    );
    attach_provider(&mut result, job.provider);
    result.outputs.push(ToolValue::text(
        segment_list.to_string(),
        "structured/audio-segments",
    ));
    result.metadata.insert("outputNames".into(), json!(names));
    result.metadata.insert("segments".into(), segment_list);
    result.metadata.insert("silence".into(), summary);
    result.warnings = warnings;
    Ok(result)
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

fn summary_json(info: &AudioInfo) -> Value {
    let stream = info.streams.first();
    json!({
        "container": info.container,
        "codec": stream.map(|stream| stream.codec.clone()),
        "durationSeconds": info.duration,
        "sampleRate": stream.map(|stream| stream.sample_rate),
        "channels": stream.map(|stream| stream.channels),
        "channelLayout": stream.and_then(|stream| stream.layout.clone()),
        "bitRate": stream.and_then(|stream| stream.bitrate).or(info.bitrate),
        "sizeBytes": info.bytes,
        "audioTracks": info.streams.len(),
        "cover": info.cover.as_ref().map(|cover| json!({ "width": cover.width, "height": cover.height })),
        "tags": info.tags,
    })
}

fn valid_tag(field: &str, value: &str) -> Result<(), String> {
    let limit = if field == "comment" { 1024 } else { 256 };
    if value.chars().count() > limit || value.chars().any(char::is_control) {
        return Err(format!(
            "{field} must be at most {limit} characters without line breaks"
        ));
    }
    let numbered = |value: &str| {
        let mut parts = value.splitn(2, '/');
        let valid = |part: Option<&str>| {
            part.is_some_and(|part| {
                !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
            })
        };
        valid(parts.next()) && parts.next().is_none_or(|total| valid(Some(total)))
    };
    match field {
        "year" => {
            let date = value.as_bytes();
            let year = date.len() >= 4 && date[..4].iter().all(u8::is_ascii_digit);
            if !(year
                && (date.len() == 4 || (date.len() == 10 && date[4] == b'-' && date[7] == b'-')))
            {
                return Err("Year must be four digits, or a full date like 2024-05-17".into());
            }
        }
        "track" | "disc" if !numbered(value) => {
            return Err(format!("{field} must be a number like 3 or 3/12"));
        }
        _ => {}
    }
    Ok(())
}

fn metadata(job: &Job, inputs: &[PathBuf]) -> Result<ToolResult, String> {
    let request = job.request;
    let input = &inputs[0];
    let ffmpeg = &job.provider.executable_path;
    let info = probe(ffmpeg, input, job.cancelled)?;
    let stream = info.first()?.clone();
    match option_str(request, "operation", "inspect")? {
        "inspect" => {
            let mut result = base_result(
                job.manifest,
                vec![ToolValue::text(
                    serde_json::to_string_pretty(&summary_json(&info)).unwrap_or_default(),
                    "structured/audio-metadata",
                )],
                &format!(
                    "{} tag{}{}",
                    info.tags.len(),
                    if info.tags.len() == 1 { "" } else { "s" },
                    if info.cover.is_some() {
                        " and cover art"
                    } else {
                        ""
                    }
                ),
            );
            attach_provider(&mut result, job.provider);
            Ok(result)
        }
        "edit" => {
            let (extension, muxer) = copy_target(&stream.codec).ok_or_else(|| {
                format!(
                    "Tag editing supports MP3, AAC/ALAC, FLAC, Ogg Vorbis, Opus, and WAV files; this one is {}",
                    stream.codec
                )
            })?;
            let only_listed = option_bool(request, "onlyListed", false)?;
            let remove_cover = option_bool(request, "removeCover", false)?;
            let new_cover = inputs.get(1);
            if new_cover.is_some() && remove_cover {
                return Err("Choose either a new cover or Remove cover, not both".into());
            }
            let holds_cover = matches!(muxer, "mp3" | "ipod" | "flac");
            if new_cover.is_some() && !holds_cover {
                return Err("Cover art can be embedded in MP3, M4A, and FLAC files".into());
            }
            if let Some(cover) = new_cover {
                let kind = cover
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if !matches!(kind.as_str(), "jpg" | "jpeg" | "png") {
                    return Err("The cover image must be JPEG or PNG".into());
                }
            }
            let mut tags = Vec::new();
            for (field, key) in TAG_FIELDS {
                match request.options.get(field) {
                    None | Some(Value::Null) => {}
                    Some(value) => {
                        let value = value
                            .as_str()
                            .ok_or_else(|| format!("{field} must be text"))?
                            .trim();
                        if !value.is_empty() {
                            valid_tag(field, value)?;
                            tags.push((key, value.to_string()));
                        }
                    }
                }
            }
            if tags.is_empty() && !only_listed && !remove_cover && new_cover.is_none() {
                return Err("Enter at least one tag, or choose a cover change".into());
            }
            // Ogg keeps comments on the stream, the other containers on the file.
            let on_stream = muxer == "ogg";
            let mut args: Vec<OsString> = vec!["-map".into(), "0:a".into()];
            let mut cover_mapped = false;
            if new_cover.is_some() {
                args.extend(["-map".into(), "1:v:0".into()]);
                cover_mapped = true;
            } else if let Some(cover) = info.cover.as_ref().filter(|_| holds_cover && !remove_cover)
            {
                args.extend(["-map".into(), format!("0:{}", cover.index).into()]);
                cover_mapped = true;
            }
            args.extend(["-c".into(), "copy".into()]);
            if cover_mapped {
                args.extend(["-disposition:v:0".into(), "attached_pic".into()]);
            }
            if only_listed {
                args.extend(["-map_metadata".into(), "-1".into()]);
                if on_stream {
                    args.extend(["-map_metadata:s:a".into(), "-1".into()]);
                }
            }
            for (key, value) in &tags {
                args.push(
                    if on_stream {
                        "-metadata:s:a:0"
                    } else {
                        "-metadata"
                    }
                    .into(),
                );
                args.push(format!("{key}={value}").into());
            }
            if muxer == "mp3" {
                args.extend(["-id3v2_version".into(), "3".into()]);
            }
            args.extend(["-f".into(), muxer.into()]);
            let mut sources = vec![MediaInput::plain(input)];
            if let Some(cover) = new_cover {
                sources.push(MediaInput::plain(cover));
            }
            let stage = stage_directory(input)?;
            let mut result = produce(
                job,
                input,
                stage.path(),
                &sources,
                args,
                extension,
                "tagged",
                info.duration,
                None,
            )?;
            let saved = result
                .outputs
                .first()
                .and_then(|output| job.grants.resolve(&output.value).ok());
            if let Some(path) = saved {
                let after = probe(ffmpeg, &path, job.cancelled)?;
                result.outputs.push(ToolValue::text(
                    serde_json::to_string_pretty(&summary_json(&after)).unwrap_or_default(),
                    "structured/audio-metadata",
                ));
            }
            let mut changes = Vec::new();
            if !tags.is_empty() {
                changes.push(format!(
                    "{} tag{}",
                    tags.len(),
                    if tags.len() == 1 { "" } else { "s" }
                ));
            }
            if new_cover.is_some() {
                changes.push("new cover".into());
            } else if remove_cover && info.cover.is_some() {
                changes.push("cover removed".into());
            }
            if only_listed {
                changes.push("other tags cleared".into());
            }
            result.message = Some(format!(
                "Saved {} ({})",
                output_name(&result),
                changes.join(", ")
            ));
            Ok(result)
        }
        other => Err(format!("Unknown metadata operation: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Previews for the waveform editors
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioPreview {
    pub duration_seconds: Option<f64>,
    pub codec: Option<String>,
    pub sample_rate: Option<u64>,
    pub channels: Option<u64>,
    pub channel_layout: Option<String>,
    pub bit_rate: Option<u64>,
    pub source_bytes: Option<u64>,
    pub container: String,
    pub tracks: usize,
    pub lossless: bool,
    /// Whether this file can be copied without re-encoding when cut or joined.
    pub copyable: bool,
    pub language: Option<String>,
    pub tags: BTreeMap<String, String>,
    pub cover: Option<String>,
    /// Peak amplitude per column, 0–1.
    pub peaks: Vec<f32>,
    /// Silent stretches `[start, end]` when silence detection was requested.
    pub silences: Option<Vec<[f64; 2]>>,
}

/// File facts, tags, cover, a peak waveform, and optionally detected silence,
/// all from one decoding pass.
pub fn audio_preview(
    grants: &FileGrants,
    token: &str,
    columns: u32,
    silence: Option<(f64, f64)>,
    cancelled: &AtomicBool,
) -> Result<AudioPreview, String> {
    let path = grants.resolve(token).map_err(|error| error.to_string())?;
    let provider = cached_provider()?;
    let ffmpeg = &provider.executable_path;
    let info = probe(ffmpeg, &path, cancelled)?;
    let stream = info.streams.first().cloned();
    let mut preview = AudioPreview {
        duration_seconds: info.duration,
        codec: stream.as_ref().map(|stream| stream.codec.clone()),
        sample_rate: stream.as_ref().map(|stream| stream.sample_rate),
        channels: stream.as_ref().map(|stream| stream.channels),
        channel_layout: stream.as_ref().and_then(|stream| stream.layout.clone()),
        bit_rate: stream
            .as_ref()
            .and_then(|stream| stream.bitrate)
            .or(info.bitrate),
        source_bytes: info.bytes,
        container: info.container.clone(),
        tracks: info.streams.len(),
        lossless: stream.as_ref().is_some_and(|stream| {
            matches!(
                stream.codec.as_str(),
                "flac" | "alac" | "wavpack" | "ape" | "tta"
            ) || stream.codec.starts_with("pcm_")
        }),
        copyable: stream
            .as_ref()
            .is_some_and(|stream| copy_target(&stream.codec).is_some()),
        language: stream.as_ref().and_then(|stream| stream.language.clone()),
        tags: info.tags.clone(),
        cover: None,
        peaks: Vec::new(),
        silences: None,
    };
    let Some(stream) = stream else {
        return Ok(preview);
    };
    let columns = columns.min(2_000);
    if columns == 0 && silence.is_none() && info.cover.is_none() {
        return Ok(preview);
    }
    let stage = tempfile::Builder::new()
        .prefix("arcade-audio-preview-")
        .tempdir()
        .map_err(|error| format!("Cannot create a preview folder: {error}"))?;
    let mut args: Vec<OsString> = Vec::new();
    if columns > 0 || silence.is_some() {
        let mut chain = Vec::new();
        if let Some((threshold, minimum)) = silence {
            chain.push(format!(
                "silencedetect=noise={:.2}dB:d={:.3}",
                threshold.clamp(-90.0, -10.0),
                minimum.clamp(0.05, 30.0)
            ));
        }
        if columns > 0 {
            let samples = info.duration.map_or(stream.sample_rate / 10, |seconds| {
                ((seconds * stream.sample_rate as f64) / columns as f64).ceil() as u64
            });
            chain.extend([
                "aformat=channel_layouts=mono".to_string(),
                format!("asetnsamples=n={}:p=0", samples.max(64)),
                "astats=metadata=1:reset=1".to_string(),
                "ametadata=mode=print:key=lavfi.astats.Overall.Peak_level:file=peaks.txt"
                    .to_string(),
            ]);
        }
        args.extend([
            "-map".into(),
            format!("0:{}", stream.index).into(),
            "-af".into(),
            chain.join(",").into(),
            "-f".into(),
            "null".into(),
            "-".into(),
        ]);
    }
    if let Some(cover) = &info.cover {
        args.extend([
            "-map".into(),
            format!("0:{}", cover.index).into(),
            "-frames:v".into(),
            "1".into(),
            "-vf".into(),
            "scale=min(160\\,iw):-2".into(),
            "-c:v".into(),
            "mjpeg".into(),
            "-q:v".into(),
            "4".into(),
            "-update".into(),
            "1".into(),
            "-f".into(),
            "image2".into(),
            "-y".into(),
            "cover.jpg".into(),
        ]);
    }
    let output = run_ffmpeg(
        ffmpeg,
        &[MediaInput::plain(&path)],
        args,
        stage.path(),
        None,
        cancelled,
        Arc::new(|_| {}),
        "info",
    )?;
    if columns > 0 {
        let text = fs::read_to_string(stage.path().join("peaks.txt")).unwrap_or_default();
        preview.peaks = text
            .lines()
            .filter_map(|line| line.split("Peak_level=").nth(1))
            .map(|value| {
                value
                    .trim()
                    .parse::<f64>()
                    .map_or(0.0, |db| 10_f64.powf(db / 20.0).clamp(0.0, 1.0) as f32)
            })
            .collect();
    }
    if silence.is_some() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let duration = info.duration.unwrap_or(f64::MAX);
        preview.silences = Some(
            silences(&stderr, duration)
                .into_iter()
                .map(|(start, end)| [start, end])
                .collect(),
        );
    }
    if let Ok(bytes) = fs::read(stage.path().join("cover.jpg")) {
        preview.cover = Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)));
    }
    Ok(preview)
}

/// EBU R128 measurement of the first audio track.
pub fn measure_audio_loudness(
    grants: &FileGrants,
    token: &str,
    cancelled: &AtomicBool,
) -> Result<LoudnessReport, String> {
    let path = grants.resolve(token).map_err(|error| error.to_string())?;
    let provider = cached_provider()?;
    let info = probe(&provider.executable_path, &path, cancelled)?;
    let stream = info.first()?;
    let output = run_ffmpeg(
        &provider.executable_path,
        &[MediaInput::plain(&path)],
        vec![
            "-map".into(),
            format!("0:{}", stream.index).into(),
            "-af".into(),
            "loudnorm=print_format=json".into(),
            "-f".into(),
            "null".into(),
            "-".into(),
        ],
        &std::env::temp_dir(),
        None,
        cancelled,
        Arc::new(|_| {}),
        "info",
    )?;
    let stats = loudnorm_json(&String::from_utf8_lossy(&output.stderr))
        .ok_or("FFmpeg did not report loudness measurements")?;
    Ok(LoudnessReport {
        integrated: loudnorm_number(&stats, "input_i"),
        true_peak: loudnorm_number(&stats, "input_tp"),
        range: loudnorm_number(&stats, "input_lra"),
        threshold: loudnorm_number(&stats, "input_thresh"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        grants::SelectedFile,
        process::{self, ProcessSpec},
        provider::{ProviderInfo, discover_ffmpeg},
    };
    use arcade_contract::ResultStatus;
    use std::time::Duration;

    /// Sound at 0–1 s and 2–3 s, silence at 1–2 s and 3–3.8 s.
    const TONE: &str = "if(lt(t\\,1)+between(t\\,2\\,3)\\,AMP*sin(2*PI*440*t)\\,0)";

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

        fn tone(&self, amplitude: &str, rate: &str) -> String {
            format!("aevalsrc={}:s={rate}:d=3.8", TONE.replace("AMP", amplitude))
        }

        fn grant(&self, name: &str) -> SelectedFile {
            self.runtime.grants().grant(&self.dir.join(name)).unwrap()
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

        fn path(&self, result: &ToolResult, index: usize) -> PathBuf {
            self.runtime
                .grants()
                .resolve(&result.outputs[index].value)
                .unwrap()
        }

        fn info(&self, result: &ToolResult, index: usize) -> AudioInfo {
            probe(
                &self.provider.executable_path,
                &self.path(result, index),
                &AtomicBool::new(false),
            )
            .unwrap()
        }

        fn length(&self, result: &ToolResult, index: usize) -> f64 {
            self.info(result, index).duration.unwrap()
        }

        fn integrated(&self, result: &ToolResult) -> f64 {
            let token = &result.outputs[0].value;
            measure_audio_loudness(self.runtime.grants(), token, &AtomicBool::new(false))
                .unwrap()
                .integrated
        }
    }

    fn near(actual: f64, expected: f64, tolerance: f64, what: &str) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{what}: expected {expected}±{tolerance}, got {actual}"
        );
    }

    #[test]
    fn silence_spans_and_audible_parts_are_complementary() {
        let log = "[silencedetect] silence_start: 1.0\n[silencedetect] silence_end: 2.0 | silence_duration: 1.0\n[silencedetect] silence_start: 3.0\n";
        let quiet = silences(log, 3.8);
        assert_eq!(quiet, vec![(1.0, 2.0), (3.0, 3.8)]);
        assert_eq!(audible(&quiet, 3.8, 0.0), vec![(0.0, 1.0), (2.0, 3.0)]);
        // Padding merges parts whose gaps close up.
        assert_eq!(audible(&quiet, 3.8, 0.6), vec![(0.0, 3.6)]);
        assert_eq!(atempo_chain(4.0).len(), 2);
        assert_eq!(atempo_chain(0.3).len(), 2);
        assert!(valid_tag("year", "2024").is_ok());
        assert!(valid_tag("year", "24").is_err());
        assert!(valid_tag("track", "3/12").is_ok());
        assert!(valid_tag("track", "three").is_err());
    }

    #[test]
    fn installed_ffmpeg_runs_every_audio_tool() {
        let Some(fx) = Fixture::new() else { return };
        if !fx.has(&[
            "encoder:libmp3lame",
            "encoder:flac",
            "filter:loudnorm",
            "filter:silencedetect",
            "filter:acrossfade",
            "filter:atrim",
        ]) {
            return;
        }
        let tone = fx.tone("0.4", "44100");
        let quiet = fx.tone("0.05", "48000");
        fx.ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=64x64",
            "-frames:v",
            "1",
            "cover.png",
        ]);
        fx.ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            &tone,
            "-i",
            "cover.png",
            "-map",
            "0:a",
            "-map",
            "1:v",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "128k",
            "-c:v",
            "png",
            "-disposition:v",
            "attached_pic",
            "-id3v2_version",
            "3",
            "-metadata",
            "title=Tone",
            "-metadata",
            "artist=Arcade",
            "tone.mp3",
        ]);
        fx.ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            &quiet,
            "-ac",
            "2",
            "-c:a",
            "pcm_s16le",
            "quiet.wav",
        ]);
        fx.ffmpeg(&[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:sample_rate=48000:duration=2",
            "-c:a",
            "flac",
            "other.flac",
        ]);
        let mp3 = fx.grant("tone.mp3");
        let wav = fx.grant("quiet.wav");
        let flac = fx.grant("other.flac");
        let cover = fx.grant("cover.png");

        // Convert keeps tags and cover art, and handles batches.
        let flac_out = fx.ok("arcade.audio.convert", &[&mp3], json!({"format": "flac"}));
        let info = fx.info(&flac_out, 0);
        assert_eq!(info.streams[0].codec, "flac");
        assert_eq!(info.tags.get("title").map(String::as_str), Some("Tone"));
        assert!(info.cover.is_some(), "cover art was dropped");
        assert_eq!(flac_out.outputs[0].mime, "file/audio");
        let batch = fx.ok(
            "arcade.audio.convert",
            &[&mp3, &wav],
            json!({"format": "m4a", "bitrateKbps": 96, "channels": "mono"}),
        );
        assert_eq!(batch.outputs.len(), 2, "{:?}", batch.warnings);
        assert_eq!(fx.info(&batch, 1).streams[0].channels, 1);
        assert!(
            fx.run(
                "arcade.audio.convert",
                &[&mp3],
                json!({"format": "mp3", "bitrateKbps": 400})
            )
            .is_err()
        );

        // Trim: lossless copy by default, re-encode for fades, cut out a middle part.
        let copy = fx.ok(
            "arcade.audio.trim",
            &[&mp3],
            json!({"startSeconds": 0.5, "endSeconds": 2.5}),
        );
        assert_eq!(copy.metadata["method"], "copy");
        near(fx.length(&copy, 0), 2.0, 0.1, "copied trim");
        let faded = fx.ok(
            "arcade.audio.trim",
            &[&wav],
            json!({"startSeconds": 0.5, "endSeconds": 2.5, "fadeInSeconds": 0.3, "fadeOutSeconds": 0.3}),
        );
        assert_eq!(faded.metadata["method"], "reencode");
        near(fx.length(&faded, 0), 2.0, 0.02, "faded trim");
        let cut = fx.ok(
            "arcade.audio.trim",
            &[&wav],
            json!({"mode": "remove", "startSeconds": 1.0, "endSeconds": 2.0}),
        );
        near(fx.length(&cut, 0), 2.79, 0.03, "cut out middle");
        assert!(fx.run("arcade.audio.trim", &[&wav], json!({"startSeconds": 1.0, "endSeconds": 2.0, "fadeInSeconds": 0.8, "fadeOutSeconds": 0.8})).is_err());

        // Join: matching clips copy, mismatched clips re-encode, crossfades overlap.
        let joined = fx.ok("arcade.audio.join", &[&mp3, &mp3], json!({}));
        assert_eq!(joined.metadata["method"], "copy");
        near(fx.length(&joined, 0), 7.6, 0.15, "copied join");
        let mixed = fx.ok("arcade.audio.join", &[&mp3, &flac], json!({}));
        assert_eq!(mixed.metadata["method"], "reencode");
        assert!(
            mixed
                .warnings
                .iter()
                .any(|warning| warning.contains("Re-encoded")),
            "{:?}",
            mixed.warnings
        );
        let crossfaded = fx.ok(
            "arcade.audio.join",
            &[&wav, &wav],
            json!({"format": "wav", "crossfadeSeconds": 0.5}),
        );
        near(fx.length(&crossfaded, 0), 7.1, 0.03, "crossfaded join");
        let gapped = fx.ok(
            "arcade.audio.join",
            &[&wav, &flac],
            json!({"format": "flac", "gapSeconds": 1.0}),
        );
        near(fx.length(&gapped, 0), 6.8, 0.03, "join with gap");

        // Normalize: two-pass loudness lands on target; peak mode sets the peak.
        let loud = fx.ok(
            "arcade.audio.normalize",
            &[&wav],
            json!({"preset": "podcast"}),
        );
        assert_eq!(loud.outputs[1].mime, "structured/audio-loudness");
        assert_eq!(fx.info(&loud, 0).streams[0].sample_rate, 48_000);
        near(fx.integrated(&loud), -16.0, 1.0, "normalized loudness");
        let peak = fx.ok(
            "arcade.audio.normalize",
            &[&wav],
            json!({"mode": "peak", "peakDb": -3.0}),
        );
        let peaks = audio_preview(
            fx.runtime.grants(),
            &peak.outputs[0].value,
            50,
            None,
            &AtomicBool::new(false),
        )
        .unwrap()
        .peaks;
        let loudest = peaks.iter().copied().fold(0.0_f32, f32::max) as f64;
        near(20.0 * loudest.log10(), -3.0, 0.2, "normalized peak");
        let batch = fx.ok(
            "arcade.audio.normalize",
            &[&wav, &mp3],
            json!({"preset": "streaming"}),
        );
        assert_eq!(batch.outputs.len(), 4, "{:?}", batch.warnings);

        // Speed and pitch.
        let fast = fx.ok("arcade.audio.speed-pitch", &[&wav], json!({"speed": 2.0}));
        near(fx.length(&fast, 0), 1.9, 0.03, "double speed");
        let slow = fx.ok(
            "arcade.audio.speed-pitch",
            &[&wav],
            json!({"speed": 0.5, "preservePitch": false}),
        );
        near(fx.length(&slow, 0), 7.6, 0.05, "half speed, pitch follows");
        let pitched = fx.ok(
            "arcade.audio.speed-pitch",
            &[&wav],
            json!({"speed": 1.0, "pitchSemitones": 3}),
        );
        near(fx.length(&pitched, 0), 3.8, 0.1, "pitch shift keeps length");

        // Silence: trim the tail, tighten pauses, split on silence.
        let options = |operation: &str| json!({"operation": operation, "thresholdDb": -50, "minimumSeconds": 0.3});
        let trimmed = fx.ok("arcade.audio.silence", &[&wav], options("trim"));
        near(fx.length(&trimmed, 0), 3.1, 0.05, "silence trim");
        let tightened = fx.ok(
            "arcade.audio.silence",
            &[&wav],
            json!({"operation": "shorten", "thresholdDb": -50, "minimumSeconds": 0.3, "keepSeconds": 0.2}),
        );
        near(fx.length(&tightened, 0), 2.3, 0.05, "shortened pauses");
        let parts = fx.ok("arcade.audio.silence", &[&mp3], options("split"));
        assert_eq!(parts.metadata["outputNames"].as_array().unwrap().len(), 2);
        assert_eq!(
            parts.outputs.last().unwrap().mime,
            "structured/audio-segments"
        );

        // Tags: inspect, edit with a new cover, and keep only the listed tags.
        let inspected = fx.ok(
            "arcade.audio.metadata",
            &[&mp3],
            json!({"operation": "inspect"}),
        );
        let summary: Value = serde_json::from_str(&inspected.outputs[0].value).unwrap();
        assert_eq!(summary["tags"]["artist"], "Arcade");
        assert!(summary["cover"].is_object());
        let tagged = fx.ok(
            "arcade.audio.metadata",
            &[&mp3, &cover],
            json!({"operation": "edit", "album": "Fixtures", "year": "2024"}),
        );
        let info = fx.info(&tagged, 0);
        assert_eq!(info.tags.get("album").map(String::as_str), Some("Fixtures"));
        assert_eq!(info.tags.get("artist").map(String::as_str), Some("Arcade"));
        assert!(info.cover.is_some());
        let only = fx.ok(
            "arcade.audio.metadata",
            &[&mp3],
            json!({"operation": "edit", "title": "Renamed", "onlyListed": true, "removeCover": true}),
        );
        let info = fx.info(&only, 0);
        assert_eq!(info.tags.get("title").map(String::as_str), Some("Renamed"));
        assert!(!info.tags.contains_key("artist"), "{:?}", info.tags);
        assert!(info.cover.is_none());
        if fx.has(&["encoder:libvorbis"]) {
            let ogg = fx.ok("arcade.audio.convert", &[&mp3], json!({"format": "ogg"}));
            assert_eq!(
                fx.info(&ogg, 0).tags.get("title").map(String::as_str),
                Some("Tone")
            );
            let file = SelectedFile {
                token: ogg.outputs[0].value.clone(),
                ..fx.runtime.grants().grant(&fx.path(&ogg, 0)).unwrap()
            };
            let retagged = fx.ok(
                "arcade.audio.metadata",
                &[&file],
                json!({"operation": "edit", "artist": "Vorbis"}),
            );
            let info = fx.info(&retagged, 0);
            assert_eq!(info.tags.get("artist").map(String::as_str), Some("Vorbis"));
            assert_eq!(info.tags.get("title").map(String::as_str), Some("Tone"));
        }

        // Previews: waveform columns, detected silence, and the cover.
        let preview = audio_preview(
            fx.runtime.grants(),
            &mp3.token,
            200,
            Some((-50.0, 0.3)),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(
            (150..=260).contains(&preview.peaks.len()),
            "{} columns",
            preview.peaks.len()
        );
        assert!(preview.peaks.iter().any(|peak| *peak > 0.2));
        assert_eq!(preview.silences.as_ref().map(Vec::len), Some(2));
        assert!(
            preview
                .cover
                .as_deref()
                .is_some_and(|url| url.starts_with("data:image/jpeg;base64,"))
        );
        assert_eq!(preview.tags.get("title").map(String::as_str), Some("Tone"));
        assert!(preview.copyable);
    }
}
