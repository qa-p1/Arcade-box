//! Speech tools: transcription and auto-subtitles through Groq's hosted
//! Whisper (`whisper-large-v3`), and offline text to speech with the voice
//! built into the operating system.
//!
//! Transcription is the only cloud step. Audio is reduced to 16 kHz mono MP3
//! and split into 30-minute chunks to stay under Groq's upload limit. The API
//! key travels in a private header file passed to curl with `-H @file`, so it
//! never appears in an argument vector, a log, or a result.

use super::job::{Job, Progress, produce, publish_named};
use super::{
    MediaInput, attach_provider, base_result, default_output_name, ffprobe_json, option_bool,
    option_str, run_ffmpeg, stage_directory,
};
use crate::{
    Arcade, network,
    process::{self, ProcessSpec},
    provider::{ProviderInfo, discover_ffmpeg, find_system_executable},
    secrets,
    tool_kit::{self, check_cancelled},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ToolValue};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

const GROQ_MODEL: &str = "whisper-large-v3";
const CHUNK_SECONDS: u32 = 1800;
const MAX_TTS_CHARS: usize = 100_000;

#[derive(Clone, Debug, PartialEq)]
struct Segment {
    start: f64,
    end: f64,
    text: String,
}

struct Transcript {
    segments: Vec<Segment>,
    text: String,
    language: Option<String>,
}

// ---------------------------------------------------------------------------
// Transcription (Groq Whisper)
// ---------------------------------------------------------------------------

/// Transcribe the first audio track of `input`. Progress runs 0..1 over
/// extraction (first 10%) and upload/recognition.
fn transcribe_file(
    ffmpeg: &ProviderInfo,
    request: &ToolRequest,
    input: &Path,
    stage: &Path,
    cancelled: &AtomicBool,
    progress: &Progress,
) -> Result<Transcript, String> {
    let key = secrets::groq_api_key().ok_or_else(|| {
        format!(
            "Speech-to-text uses Groq. Add your key as GROQ_API_KEY in {}",
            secrets::env_file_hint()
        )
    })?;
    let translate = option_str(request, "task", "transcribe")? == "translate";
    let language = match option_str(request, "language", "auto")? {
        "auto" | "" => None,
        code if code.len() <= 3 && code.bytes().all(|byte| byte.is_ascii_lowercase()) => Some(code),
        other => return Err(format!("Unknown language code: {other}")),
    };
    let duration = ffprobe_json(&ffmpeg.executable_path, input, cancelled)?["format"]["duration"]
        .as_str()
        .and_then(|value| value.parse::<f64>().ok());
    let extract = progress.clone();
    run_ffmpeg(
        &ffmpeg.executable_path,
        &[MediaInput::plain(input)],
        [
            "-map",
            "0:a:0",
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "32k",
            "-f",
            "segment",
            "-segment_time",
            &CHUNK_SECONDS.to_string(),
            "-reset_timestamps",
            "1",
            "-segment_list",
            "chunks.csv",
            "-segment_list_type",
            "csv",
            "chunk-%03d.mp3",
        ]
        .into_iter()
        .map(OsString::from)
        .collect(),
        stage,
        duration,
        cancelled,
        Arc::new(move |fraction| extract(fraction * 0.1)),
        "error",
    )
    .map_err(|error| {
        if error.contains("matches no streams") {
            "This file has no audio track to transcribe".to_owned()
        } else {
            error
        }
    })?;
    // chunks.csv rows: file name, start seconds, end seconds.
    let list = fs::read_to_string(stage.join("chunks.csv")).map_err(|error| error.to_string())?;
    let chunks = list
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(',');
            let name = fields.next()?.trim().to_owned();
            let start = fields.next()?.trim().parse::<f64>().ok()?;
            (name.starts_with("chunk-") && name.ends_with(".mp3")).then_some((name, start))
        })
        .collect::<Vec<_>>();
    if chunks.is_empty() {
        return Err("No audio could be extracted from this file".into());
    }
    write_auth_header(stage, &key)?;
    drop(key);
    let curl = network::curl_provider()?;
    let endpoint = if translate {
        "translations"
    } else {
        "transcriptions"
    };
    let mut transcript = Transcript {
        segments: Vec::new(),
        text: String::new(),
        language: None,
    };
    for (position, (name, offset)) in chunks.iter().enumerate() {
        check_cancelled(cancelled)?;
        let response = groq_request(&curl, stage, name, endpoint, language, cancelled)?;
        for segment in response["segments"].as_array().into_iter().flatten() {
            let text = segment["text"].as_str().unwrap_or_default().trim();
            if text.is_empty() {
                continue;
            }
            transcript.segments.push(Segment {
                start: offset + segment["start"].as_f64().unwrap_or(0.0),
                end: offset + segment["end"].as_f64().unwrap_or(0.0),
                text: text.to_owned(),
            });
        }
        let text = response["text"].as_str().unwrap_or_default().trim();
        if !text.is_empty() {
            if !transcript.text.is_empty() {
                transcript.text.push(' ');
            }
            transcript.text.push_str(text);
        }
        if transcript.language.is_none() {
            transcript.language = response["language"].as_str().map(str::to_owned);
        }
        progress(0.1 + 0.9 * (position + 1) as f64 / chunks.len() as f64);
    }
    let _ = fs::remove_file(stage.join("auth.txt"));
    if transcript.text.is_empty() {
        return Err("No speech was recognised in this file".into());
    }
    Ok(transcript)
}

/// Transcribe any audio file to `(start, end, text)` segments, for tools
/// outside the media module (such as video transcripts without captions).
pub(crate) fn transcribe_to_segments(
    request: &ToolRequest,
    input: &Path,
    stage: &Path,
    cancelled: &AtomicBool,
    progress: Progress,
) -> Result<Vec<(f64, f64, String)>, String> {
    let ffmpeg = discover_ffmpeg(None)
        .into_iter()
        .find(|provider| {
            provider.compatible
                && provider
                    .capabilities
                    .iter()
                    .any(|cap| cap == "encoder:libmp3lame")
        })
        .ok_or("Transcribing needs FFmpeg with MP3 support")?;
    let transcript = transcribe_file(&ffmpeg, request, input, stage, cancelled, &progress)?;
    Ok(transcript
        .segments
        .into_iter()
        .map(|segment| (segment.start, segment.end, segment.text))
        .collect())
}

/// The Authorization header in a private file for `curl -H @auth.txt`.
fn write_auth_header(stage: &Path, key: &str) -> Result<(), String> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(stage.join("auth.txt"))
        .map_err(|error| format!("Could not prepare the request: {error}"))?;
    file.write_all(format!("Authorization: Bearer {key}\n").as_bytes())
        .map_err(|error| format!("Could not prepare the request: {error}"))
}

fn groq_request(
    curl: &ProviderInfo,
    stage: &Path,
    chunk: &str,
    endpoint: &str,
    language: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Value, String> {
    let mut args = network::curl_common_args();
    let mut form = vec![
        format!("model={GROQ_MODEL}"),
        "response_format=verbose_json".into(),
        "temperature=0".into(),
        format!("file=@{chunk}"),
    ];
    if endpoint == "transcriptions" {
        form.push("timestamp_granularities[]=segment".into());
        if let Some(language) = language {
            form.push(format!("language={language}"));
        }
    }
    args.extend([
        "--max-time".into(),
        "900".into(),
        "-H".into(),
        "@auth.txt".into(),
    ]);
    for field in form {
        args.extend(["-F".into(), field.into()]);
    }
    args.extend([
        "--write-out".into(),
        "\n%{http_code}".into(),
        "--".into(),
        format!("https://api.groq.com/openai/v1/audio/{endpoint}").into(),
    ]);
    let output = process::run(
        &ProcessSpec {
            executable: curl.executable_path.clone(),
            args,
            current_dir: Some(stage.to_path_buf()),
            timeout: Duration::from_secs(960),
            output_limit: 32 * 1024 * 1024,
        },
        cancelled,
    )
    .map_err(|error| format!("Could not reach Groq: {error}"))?;
    if !output.status.success() {
        return Err(network::process_error(
            &output.stderr,
            "Could not reach Groq",
        ));
    }
    let body = String::from_utf8_lossy(&output.stdout);
    let (json_text, status) = body.rsplit_once('\n').unwrap_or((&body, ""));
    let value: Value = serde_json::from_str(json_text).unwrap_or(Value::Null);
    match status.trim() {
        "200" => Ok(value),
        code => {
            let reason = value["error"]["message"].as_str().unwrap_or("no details");
            Err(match code {
                "401" => "Groq rejected the API key. Check GROQ_API_KEY.".into(),
                "413" => "This audio chunk is too large for Groq".into(),
                "429" => format!("Groq's rate limit was reached; try again shortly ({reason})"),
                _ => format!("Groq returned HTTP {code}: {reason}"),
            })
        }
    }
}

/// Split long recognised segments into readable subtitle cues of at most two
/// lines of about 42 characters, sharing time in proportion to text length.
fn cues(segments: &[Segment]) -> Vec<Segment> {
    const MAX_CUE: usize = 84;
    let mut cues = Vec::new();
    for segment in segments {
        let mut parts: Vec<String> = Vec::new();
        for word in segment.text.split_whitespace() {
            match parts.last_mut() {
                Some(last) if last.len() + 1 + word.len() <= MAX_CUE => {
                    last.push(' ');
                    last.push_str(word);
                }
                _ => parts.push(word.to_owned()),
            }
        }
        let total = parts.iter().map(String::len).sum::<usize>().max(1) as f64;
        let span = (segment.end - segment.start).max(0.0);
        let mut start = segment.start;
        for part in parts {
            let end = start + span * part.len() as f64 / total;
            cues.push(Segment {
                start,
                end,
                text: wrap_two_lines(&part),
            });
            start = end;
        }
    }
    cues
}

fn wrap_two_lines(text: &str) -> String {
    if text.len() <= 42 {
        return text.to_owned();
    }
    let middle = text.len() / 2;
    let split = text
        .match_indices(' ')
        .map(|(index, _)| index)
        .min_by_key(|index| index.abs_diff(middle));
    match split {
        Some(index) => format!("{}\n{}", &text[..index], &text[index + 1..]),
        None => text.to_owned(),
    }
}

fn timestamp(seconds: f64, separator: char) -> String {
    let millis = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        millis / 3_600_000,
        millis / 60_000 % 60,
        millis / 1000 % 60,
        millis % 1000
    )
}

fn subtitles(segments: &[Segment], vtt: bool) -> String {
    let separator = if vtt { '.' } else { ',' };
    let mut out = if vtt {
        "WEBVTT\n\n".to_owned()
    } else {
        String::new()
    };
    for (index, cue) in cues(segments).iter().enumerate() {
        if !vtt {
            out.push_str(&format!("{}\n", index + 1));
        }
        out.push_str(&format!(
            "{} --> {}\n{}\n\n",
            timestamp(cue.start, separator),
            timestamp(cue.end.max(cue.start + 0.3), separator),
            cue.text
        ));
    }
    out
}

fn timestamped_text(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|segment| {
            let seconds = segment.start.max(0.0) as u64;
            let stamp = if seconds >= 3600 {
                format!(
                    "{}:{:02}:{:02}",
                    seconds / 3600,
                    seconds / 60 % 60,
                    seconds % 60
                )
            } else {
                format!("{:02}:{:02}", seconds / 60, seconds % 60)
            };
            format!("[{stamp}] {}", segment.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn stage_subtitle(
    stage: &Path,
    transcript: &Transcript,
    vtt: bool,
) -> Result<std::path::PathBuf, String> {
    let path = stage.join(if vtt {
        "subtitles.vtt"
    } else {
        "subtitles.srt"
    });
    fs::write(&path, subtitles(&transcript.segments, vtt)).map_err(|error| error.to_string())?;
    Ok(path)
}

pub(super) fn transcribe(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let stage = stage_directory(input)?;
    let transcript = transcribe_file(
        job.provider,
        job.request,
        input,
        stage.path(),
        job.cancelled,
        &job.progress,
    )?;
    let text = if option_bool(job.request, "timestamps", false)? {
        timestamped_text(&transcript.segments)
    } else {
        transcript.text.clone()
    };
    let mut outputs = vec![ToolValue::text(text, "text/plain")];
    let format = option_str(job.request, "subtitles", "srt")?;
    if format != "none" {
        let vtt = format == "vtt";
        let staged = stage_subtitle(stage.path(), &transcript, vtt)?;
        let name = default_output_name(input, "", if vtt { "vtt" } else { "srt" });
        outputs.extend(publish_named(job, input, &staged, &name, Some("file/subtitle"))?.outputs);
    }
    let words = transcript.text.split_whitespace().count();
    let mut result = base_result(
        job.manifest,
        outputs,
        &format!(
            "Transcribed {words} words{}",
            transcript
                .language
                .as_deref()
                .map(|language| format!(" ({language})"))
                .unwrap_or_default()
        ),
    );
    attach_provider(&mut result, job.provider);
    result
        .metadata
        .insert("speechModel".into(), json!(format!("Groq {GROQ_MODEL}")));
    Ok(result)
}

pub(super) fn auto_subtitles(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let request = job.request;
    let mode = option_str(request, "mode", "embed")?;
    let stage = stage_directory(input)?;
    let overall = job.progress.clone();
    let transcribing: Progress = Arc::new(move |fraction| overall(fraction * 0.6));
    let transcript = transcribe_file(
        job.provider,
        request,
        input,
        stage.path(),
        job.cancelled,
        &transcribing,
    )?;
    let srt = stage_subtitle(stage.path(), &transcript, false)?;
    let srt_name = default_output_name(input, "", "srt");
    let mut outputs = publish_named(job, input, &srt, &srt_name, Some("file/subtitle"))?.outputs;
    let duration =
        ffprobe_json(&job.provider.executable_path, input, job.cancelled)?["format"]["duration"]
            .as_str()
            .and_then(|value| value.parse::<f64>().ok());
    let overall = job.progress.clone();
    let encoding = job.with_progress(Arc::new(move |fraction| overall(0.6 + fraction * 0.4)));
    let extension = input
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let video = match mode {
        "file" => None,
        "embed" => {
            let (extension, codec) = match extension.as_str() {
                "mp4" | "m4v" | "mov" => (extension.as_str(), "mov_text"),
                "webm" => ("webm", "webvtt"),
                _ => ("mkv", "srt"),
            };
            let args: Vec<OsString> = [
                "-map",
                "0:v",
                "-map",
                "0:a?",
                "-map",
                "1:0",
                "-c:v",
                "copy",
                "-c:a",
                "copy",
                "-c:s",
                codec,
                "-disposition:s:0",
                "default",
            ]
            .into_iter()
            .map(OsString::from)
            .collect();
            Some(produce(
                &encoding,
                input,
                stage.path(),
                &[MediaInput::plain(input), MediaInput::plain(&srt)],
                args,
                extension,
                "subtitled",
                duration,
                Some("file/video"),
            )?)
        }
        "burn" => {
            let size = match option_str(request, "textSize", "medium")? {
                "small" => 16,
                "medium" => 22,
                "large" => 30,
                other => return Err(format!("Unknown text size: {other}")),
            };
            // The subtitle file has a fixed relative name in the working folder,
            // so no user path enters the filtergraph.
            let args: Vec<OsString> = vec![
                "-map".into(),
                "0:v:0".into(),
                "-map".into(),
                "0:a?".into(),
                "-vf".into(),
                format!("subtitles=subtitles.srt:force_style='FontSize={size},Outline=1,Shadow=0'")
                    .into(),
                "-c:v".into(),
                "libx264".into(),
                "-crf".into(),
                "20".into(),
                "-preset".into(),
                "medium".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
                "-c:a".into(),
                "aac".into(),
                "-b:a".into(),
                "192k".into(),
                "-movflags".into(),
                "+faststart".into(),
            ];
            Some(produce(
                &encoding,
                input,
                stage.path(),
                &[MediaInput::plain(input)],
                args,
                "mp4",
                "subtitled",
                duration,
                Some("file/video"),
            )?)
        }
        other => return Err(format!("Unknown subtitle mode: {other}")),
    };
    if let Some(video) = video {
        outputs.splice(0..0, video.outputs);
    }
    let mut result = base_result(
        job.manifest,
        outputs,
        &format!(
            "Created {}{}",
            match cues(&transcript.segments).len() {
                1 => "1 subtitle".to_owned(),
                count => format!("{count} subtitles"),
            },
            match mode {
                "embed" => " and added them as a subtitle track",
                "burn" => " and burned them into the video",
                _ => "",
            }
        ),
    );
    attach_provider(&mut result, job.provider);
    result
        .metadata
        .insert("speechModel".into(), json!(format!("Groq {GROQ_MODEL}")));
    Ok(result)
}

// ---------------------------------------------------------------------------
// Text to speech (the system's own voice, offline)
// ---------------------------------------------------------------------------

/// The system speech engine: SAPI through PowerShell on Windows, `say` on
/// macOS, eSpeak NG (or eSpeak) on Linux. Nothing is bundled.
pub fn system_speech() -> Option<std::path::PathBuf> {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot")?;
        let path =
            std::path::PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        path.is_file().then_some(path)
    } else if cfg!(target_os = "macos") {
        find_system_executable("say")
    } else {
        find_system_executable("espeak-ng").or_else(|| find_system_executable("espeak"))
    }
}

/// Arguments that make `engine` read `text_file` into `wav`.
fn speech_args(
    engine: &Path,
    text_file: &Path,
    wav: &Path,
    voice: &str,
    speed: f64,
) -> Vec<OsString> {
    let words_per_minute = (175.0 * speed).round().clamp(80.0, 450.0);
    if cfg!(windows) {
        let quote = |path: &Path| path.display().to_string().replace('\'', "''");
        let gender = match voice {
            "female" => "$s.SelectVoiceByHints([System.Speech.Synthesis.VoiceGender]::Female);",
            "male" => "$s.SelectVoiceByHints([System.Speech.Synthesis.VoiceGender]::Male);",
            _ => "",
        };
        let rate = ((speed.ln() / 3f64.ln()) * 10.0).round().clamp(-10.0, 10.0);
        let script = format!(
            "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; {gender} $s.Rate = {rate}; $s.SetOutputToWaveFile('{}'); $s.Speak((Get-Content -Raw -Encoding UTF8 -LiteralPath '{}')); $s.Dispose()",
            quote(wav),
            quote(text_file),
        );
        return vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            script.into(),
        ];
    }
    let mut args: Vec<OsString> = Vec::new();
    if cfg!(target_os = "macos") {
        args.extend(["-f".into(), text_file.into(), "-o".into(), wav.into()]);
        args.extend([
            "--file-format=WAVE".into(),
            "--data-format=LEI16@22050".into(),
        ]);
        args.extend(["-r".into(), words_per_minute.to_string().into()]);
        let name = match voice {
            "female" => Some("Samantha"),
            "male" => Some("Daniel"),
            _ => None,
        };
        if let Some(name) = name.filter(|name| mac_voice_installed(engine, name)) {
            args.extend(["-v".into(), name.into()]);
        }
    } else {
        let voice = match voice {
            "female" => "en-us+f3",
            "male" => "en-us+m3",
            _ => "en-us",
        };
        args.extend(["-b".into(), "1".into(), "-f".into(), text_file.into()]);
        args.extend(["-w".into(), wav.into(), "-v".into(), voice.into()]);
        args.extend(["-s".into(), words_per_minute.to_string().into()]);
    }
    args
}

fn mac_voice_installed(say: &Path, name: &str) -> bool {
    process::run(
        &ProcessSpec {
            executable: say.to_path_buf(),
            args: vec!["-v".into(), "?".into()],
            current_dir: None,
            timeout: Duration::from_secs(10),
            output_limit: 256 * 1024,
        },
        &AtomicBool::new(false),
    )
    .is_ok_and(|output| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.starts_with(&format!("{name} ")))
    })
}

pub fn text_to_speech(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let text = tool_kit::single_text(request, runtime, MAX_TTS_CHARS * 4, cancelled)?;
    let text = text.trim();
    if text.is_empty() {
        return Err("Type the text to read aloud".into());
    }
    if text.chars().count() > MAX_TTS_CHARS {
        return Err(format!(
            "Text to speech handles up to {MAX_TTS_CHARS} characters at a time"
        ));
    }
    let voice = tool_kit::option_str(request, "voice", "default");
    if !matches!(voice, "default" | "female" | "male") {
        return Err("Choose the default, a female or a male voice".into());
    }
    let speed = tool_kit::number_in(request, "speed", "Speed", Some(1.0), 0.5..=2.0)?;
    let format = tool_kit::option_str(request, "format", "mp3");
    let engine = system_speech().ok_or(if cfg!(target_os = "linux") {
        "Text to speech uses your system voice, eSpeak NG. Install `espeak-ng` from your package manager."
    } else {
        "This system has no built-in speech engine"
    })?;
    let stage =
        tempfile::tempdir_in(runtime.artifact_staging_root()).map_err(|error| error.to_string())?;
    let text_file = stage.path().join("speech.txt");
    fs::write(&text_file, text).map_err(|error| error.to_string())?;
    let wav = stage.path().join("speech.wav");
    // PowerShell and the speech services need the user's profile folders.
    let environment: Vec<(OsString, OsString)> = [
        "HOME",
        "USERPROFILE",
        "TEMP",
        "TMP",
        "APPDATA",
        "LOCALAPPDATA",
        "PSModulePath",
        "windir",
    ]
    .into_iter()
    .filter_map(|key| std::env::var_os(key).map(|value| (key.into(), value)))
    .collect();
    let output = process::run_with_env(
        &ProcessSpec {
            executable: engine.clone(),
            args: speech_args(&engine, &text_file, &wav, voice, speed),
            current_dir: Some(stage.path().to_path_buf()),
            timeout: Duration::from_secs(30 * 60),
            output_limit: 1024 * 1024,
        },
        cancelled,
        &environment,
    )
    .map_err(|error| format!("The system voice could not run: {error}"))?;
    if !output.status.success() || !wav.is_file() {
        return Err(format!(
            "The system voice failed: {}",
            tool_kit::stderr_excerpt(&output.stderr)
        ));
    }
    let seconds = wav_seconds(&wav).unwrap_or(0.0);
    let (staged, extension) = match format {
        "wav" => (wav, "wav"),
        "mp3" | "m4a" | "ogg" => {
            let ffmpeg = discover_ffmpeg(None)
                .into_iter()
                .find(|provider| provider.compatible)
                .ok_or("Saving as MP3/M4A/Ogg needs FFmpeg; choose WAV instead")?;
            let encoded = stage.path().join(format!("speech.{format}"));
            let codec: &[&str] = match format {
                "mp3" => &["-c:a", "libmp3lame", "-b:a", "128k"],
                "m4a" => &["-c:a", "aac", "-b:a", "128k"],
                _ => &["-c:a", "libopus", "-b:a", "64k"],
            };
            let mut args = codec.iter().map(OsString::from).collect::<Vec<_>>();
            args.extend(["-n".into(), encoded.as_os_str().to_os_string()]);
            run_ffmpeg(
                &ffmpeg.executable_path,
                &[MediaInput::plain(&wav)],
                args,
                stage.path(),
                None,
                cancelled,
                Arc::new(|_| {}),
                "error",
            )?;
            (encoded, format)
        }
        other => return Err(format!("Unknown format: {other}")),
    };
    let name = tool_kit::output_name(request, &format!("speech.{extension}"))?;
    let name = if name.ends_with(&format!(".{extension}")) {
        name
    } else {
        format!("{name}.{extension}")
    };
    let published = tool_kit::publish_file(request, runtime, &staged, &name, cancelled)?;
    Ok(tool_kit::success(
        manifest,
        vec![published],
        Some(format!("Saved {} seconds of speech", seconds.round())),
        vec![],
    ))
}

/// Length of a PCM WAV file from its header.
fn wav_seconds(path: &Path) -> Option<f64> {
    let bytes = fs::read(path).ok()?;
    let header = bytes.get(..44)?;
    let channels = u16::from_le_bytes([header[22], header[23]]) as f64;
    let rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]) as f64;
    let bits = u16::from_le_bytes([header[34], header[35]]) as f64;
    let per_second = channels * rate * bits / 8.0;
    (per_second > 0.0).then(|| (bytes.len() - 44) as f64 / per_second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitles_split_long_segments_and_format_times() {
        let segments = vec![Segment {
            start: 61.5,
            end: 71.5,
            text: "This is a fairly long sentence that should be split into more than one subtitle cue because it is long.".into(),
        }];
        let srt = subtitles(&segments, false);
        assert!(srt.starts_with("1\n00:01:01,500 --> "));
        assert!(srt.contains("\n\n2\n"));
        assert!(srt.lines().all(|line| line.len() <= 60));
        let vtt = subtitles(&segments, true);
        assert!(vtt.starts_with("WEBVTT\n\n00:01:01.500 --> "));
        assert_eq!(
            timestamped_text(&segments),
            format!("[01:01] {}", segments[0].text)
        );
    }
}
