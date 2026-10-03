//! Clean-up actions: background-noise removal (audio or a video's soundtrack)
//! and vocal separation for karaoke. Noise removal prefers the RNNoise voice
//! model when the user installed it; vocal separation prefers Demucs and falls
//! back to FFmpeg's centre-channel method.

use super::{AudioInfo, Output, ensure_encoder, has_filter, keep_cover, metadata_args, probe};
use crate::media::job::{Job, output_name, produce, publish_named};
use crate::media::{
    MediaInput, ffprobe_json, option_bool, option_str, run_ffmpeg, stage_directory,
};
use crate::{
    process::{self, ProcessSpec},
    provider::{find_system_executable, user_model_file},
};
use arcade_contract::ToolResult;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// Video containers whose video can be copied untouched while the
/// soundtrack is re-encoded.
const VIDEO_CONTAINERS: [&str; 5] = ["mp4", "m4v", "mov", "mkv", "webm"];

fn has_picture(job: &Job, input: &Path) -> Result<bool, String> {
    let value = ffprobe_json(&job.provider.executable_path, input, job.cancelled)?;
    Ok(value["streams"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|stream| {
            stream["codec_type"] == "video"
                && stream["disposition"]["attached_pic"].as_u64() != Some(1)
        }))
}

// ---------------------------------------------------------------------------
// Noise removal
// ---------------------------------------------------------------------------

pub(super) fn denoise(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let request = job.request;
    let strength = option_str(request, "strength", "medium")?;
    let (mix, reduction) = match strength {
        "light" => (0.5, 10),
        "medium" => (0.8, 18),
        "strong" => (1.0, 28),
        other => return Err(format!("Unknown strength: {other}")),
    };
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    let stream = info.first()?.clone();
    let stage = stage_directory(input)?;
    let mut warnings = Vec::new();
    let mut filters = Vec::new();
    if option_bool(request, "rumble", true)? {
        filters.push("highpass=f=70".to_owned());
    }
    let method = match option_str(request, "method", "speech")? {
        "speech" => {
            match user_model_file("rnnoise/sh.rnnn").filter(|_| has_filter(job, "arnndn")) {
                Some(model) => {
                    // A fixed relative name avoids filtergraph escaping of user paths.
                    fs::copy(&model, stage.path().join("voice.rnnn"))
                        .map_err(|error| format!("Could not stage the voice model: {error}"))?;
                    // RNNoise can overshoot full scale, which crashes LAME; the
                    // limiter keeps peaks just below it.
                    filters.push(format!(
                        "arnndn=m=voice.rnnn:mix={mix},alimiter=limit=0.97:level=false"
                    ));
                    "RNNoise voice model"
                }
                None => {
                    warnings.push("The RNNoise voice model isn't installed, so the general noise filter was used.".into());
                    filters.push(format!("afftdn=nr={reduction}:nf=-40:tn=1"));
                    "FFT noise filter"
                }
            }
        }
        "general" => {
            filters.push(format!("afftdn=nr={reduction}:nf=-40:tn=1"));
            "FFT noise filter"
        }
        other => return Err(format!("Unknown method: {other}")),
    };
    let filter = filters.join(",");
    let mut result = if has_picture(job, input)? {
        denoise_video(job, input, stage.path(), &info, &filter, &mut warnings)?
    } else {
        let (output, more) = Output::resolve(request, "same", &stream)?;
        warnings.extend(more);
        ensure_encoder(job, output.encoding)?;
        let mut args: Vec<OsString> = vec![
            "-map".into(),
            format!("0:{}", stream.index).into(),
            "-af".into(),
            filter.into(),
        ];
        keep_cover(output.encoding, &info, &mut args, &mut warnings);
        args.extend(metadata_args(&info));
        args.extend(output.codec_args(&stream, true));
        produce(
            job,
            input,
            stage.path(),
            &[MediaInput::plain(input)],
            args,
            output.encoding.extension(),
            "clean",
            info.duration,
            None,
        )?
    };
    result.message = Some(format!(
        "Saved {} (cleaned with the {method}, {strength})",
        output_name(&result)
    ));
    result.warnings = warnings;
    Ok(result)
}

fn denoise_video(
    job: &Job,
    input: &Path,
    stage: &Path,
    info: &AudioInfo,
    filter: &str,
    warnings: &mut Vec<String>,
) -> Result<ToolResult, String> {
    let stream = info.first()?;
    let source_extension = input
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let extension = if VIDEO_CONTAINERS.contains(&source_extension.as_str()) {
        source_extension.as_str()
    } else {
        warnings
            .push("Saved as MKV so the original video could be kept without re-encoding.".into());
        "mkv"
    };
    if info.streams.len() > 1 {
        warnings.push("Only the first audio track was cleaned and kept.".into());
    }
    let (codec, bitrate) = if extension == "webm" {
        ("libopus", "160k")
    } else {
        ("aac", "192k")
    };
    let mut args: Vec<OsString> = vec![
        "-map".into(),
        "0:V?".into(),
        "-map".into(),
        format!("0:{}", stream.index).into(),
        "-map".into(),
        "0:s?".into(),
        "-c:v".into(),
        "copy".into(),
        "-c:s".into(),
        "copy".into(),
        "-af".into(),
        filter.into(),
        "-c:a".into(),
        codec.into(),
        "-b:a".into(),
        bitrate.into(),
    ];
    if matches!(extension, "mp4" | "m4v" | "mov") {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    produce(
        job,
        input,
        stage,
        &[MediaInput::plain(input)],
        args,
        extension,
        "clean",
        info.duration,
        None,
    )
}

// ---------------------------------------------------------------------------
// Vocal separation
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Stem {
    Instrumental,
    Vocals,
}

impl Stem {
    fn suffix(self) -> &'static str {
        match self {
            Self::Instrumental => "instrumental",
            Self::Vocals => "vocals",
        }
    }

    fn demucs_file(self) -> &'static str {
        match self {
            Self::Instrumental => "no_vocals.wav",
            Self::Vocals => "vocals.wav",
        }
    }

    /// FFmpeg's centre-channel approximation. Instrumental removes the
    /// centre (where vocals usually sit) but restores the bass below 120 Hz;
    /// vocals keep only the centre's voice band.
    fn basic_graph(self, index: usize) -> String {
        match self {
            Self::Instrumental => format!(
                "[0:{index}]asplit=2[full][mid];[full]lowpass=f=120[low];[mid]stereotools=mlev=0.015625,highpass=f=120[side];[low][side]amix=inputs=2:normalize=0[out]"
            ),
            Self::Vocals => {
                format!("[0:{index}]stereotools=slev=0.015625,highpass=f=120,lowpass=f=8000[out]")
            }
        }
    }
}

pub(super) fn vocals(job: &Job, input: &Path) -> Result<ToolResult, String> {
    let request = job.request;
    let stems = match option_str(request, "keep", "instrumental")? {
        "instrumental" => vec![Stem::Instrumental],
        "vocals" => vec![Stem::Vocals],
        "both" => vec![Stem::Instrumental, Stem::Vocals],
        other => return Err(format!("Unknown choice: {other}")),
    };
    let info = probe(&job.provider.executable_path, input, job.cancelled)?;
    let stream = info.first()?.clone();
    let (output, mut warnings) = Output::resolve(request, "mp3", &stream)?;
    ensure_encoder(job, output.encoding)?;
    // Several files can't share one requested name.
    let mut item_request = request.clone();
    if stems.len() > 1
        && let Some(options) = item_request.options.as_object_mut()
    {
        options.remove("outputName");
    }
    let job = Job {
        request: &item_request,
        ..job.with_progress(job.progress.clone())
    };
    let stage = stage_directory(input)?;
    let demucs = match option_str(request, "engine", "auto")? {
        "basic" => None,
        _ => find_system_executable("demucs"),
    };
    let mut outputs = Vec::new();
    let engine;
    if let Some(demucs) = demucs {
        engine = "Demucs AI separation";
        let separated = run_demucs(&job, &demucs, input, stage.path(), &info)?;
        for (position, stem) in stems.iter().enumerate() {
            let source = separated.join(stem.demucs_file());
            let source_info = probe(&job.provider.executable_path, &source, job.cancelled)?;
            let source_stream = source_info.first()?.clone();
            let mut args: Vec<OsString> = vec!["-map".into(), "0:a:0".into()];
            args.extend(output.codec_args(&source_stream, false));
            let item = stem_job(
                &job,
                0.7 + 0.3 * position as f64 / stems.len() as f64,
                0.3 / stems.len() as f64,
            );
            let staged =
                stage
                    .path()
                    .join(format!("{}.{}", stem.suffix(), output.encoding.extension()));
            args.extend(["-n".into(), staged.as_os_str().to_os_string()]);
            run_ffmpeg(
                &job.provider.executable_path,
                &[MediaInput::plain(&source)],
                args,
                stage.path(),
                source_info.duration,
                job.cancelled,
                item.progress.clone(),
                "error",
            )?;
            let name = crate::media::requested_output_name(
                job.request,
                input,
                stem.suffix(),
                output.encoding.extension(),
            )?;
            outputs.extend(publish_named(&job, input, &staged, &name, None)?.outputs);
        }
    } else {
        if stream.channels < 2 {
            return Err(
                "This recording is mono, so vocals can't be separated without the Demucs AI model"
                    .into(),
            );
        }
        engine = "basic centre-channel method";
        warnings.push("Used the basic method: it works best on studio mixes with centred vocals. Install Demucs for much cleaner results.".into());
        for (position, stem) in stems.iter().enumerate() {
            let item = stem_job(
                &job,
                position as f64 / stems.len() as f64,
                1.0 / stems.len() as f64,
            );
            let mut args: Vec<OsString> = vec![
                "-filter_complex".into(),
                stem.basic_graph(stream.index).into(),
                "-map".into(),
                "[out]".into(),
            ];
            args.extend(output.codec_args(&stream, false));
            outputs.extend(
                produce(
                    &item,
                    input,
                    stage.path(),
                    &[MediaInput::plain(input)],
                    args,
                    output.encoding.extension(),
                    stem.suffix(),
                    info.duration,
                    None,
                )?
                .outputs,
            );
        }
    }
    let mut result = crate::media::base_result(
        job.manifest,
        outputs,
        &format!(
            "Saved {} using the {engine}",
            stems
                .iter()
                .map(|stem| stem.suffix())
                .collect::<Vec<_>>()
                .join(" and ")
        ),
    );
    crate::media::attach_provider(&mut result, job.provider);
    result.warnings = warnings;
    Ok(result)
}

fn stem_job<'a>(job: &'a Job, from: f64, span: f64) -> Job<'a> {
    let progress = job.progress.clone();
    job.with_progress(Arc::new(move |fraction| progress(from + fraction * span)))
}

/// Decode to WAV, then run Demucs' two-stem model. Returns the folder that
/// holds `vocals.wav` and `no_vocals.wav`.
fn run_demucs(
    job: &Job,
    demucs: &Path,
    input: &Path,
    stage: &Path,
    info: &AudioInfo,
) -> Result<PathBuf, String> {
    let wav = stage.join("source.wav");
    run_ffmpeg(
        &job.provider.executable_path,
        &[MediaInput::plain(input)],
        vec![
            "-map".into(),
            "0:a:0".into(),
            "-c:a".into(),
            "pcm_s16le".into(),
            "-n".into(),
            wav.as_os_str().to_os_string(),
        ],
        stage,
        info.duration,
        job.cancelled,
        stem_job(job, 0.0, 0.05).progress.clone(),
        "error",
    )?;
    // Demucs caches its model under HOME; nothing else from the environment is passed.
    let mut environment = Vec::new();
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        environment.push((OsString::from("HOME"), home));
    }
    let output = process::run_with_env(
        &ProcessSpec {
            executable: demucs.to_path_buf(),
            args: vec![
                "--two-stems".into(),
                "vocals".into(),
                "-n".into(),
                "htdemucs".into(),
                "-o".into(),
                stage.as_os_str().to_os_string(),
                "--filename".into(),
                "{stem}.{ext}".into(),
                wav.as_os_str().to_os_string(),
            ],
            current_dir: Some(stage.to_path_buf()),
            timeout: Duration::from_secs(6 * 60 * 60),
            output_limit: 4 * 1024 * 1024,
        },
        job.cancelled,
        &environment,
    )
    .map_err(|error| format!("Demucs could not run: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Demucs failed: {}",
            stderr
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("unknown error")
        ));
    }
    (job.progress)(0.7);
    let folder = stage.join("htdemucs");
    if folder.join("vocals.wav").is_file() && folder.join("no_vocals.wav").is_file() {
        Ok(folder)
    } else {
        Err("Demucs finished but did not write the expected stems".into())
    }
}
