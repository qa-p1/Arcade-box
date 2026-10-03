//! yt-dlp shared setup plus the tools built on it besides the media
//! downloader: video transcripts (captions) and thumbnails.

use super::{PROCESS_TIMEOUT, list_stage_outputs, stage_dir, url_input};
use crate::{
    Arcade, network,
    process::{self, ProcessSpec},
    provider::{self, ProviderInfo},
    tool_kit::{check_cancelled, option_bool, option_str, output_name, publish_file, success},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ToolValue};
use std::{ffi::OsString, fs, path::Path, sync::Arc, sync::atomic::AtomicBool};

pub(super) struct YtDlp {
    pub(super) provider: ProviderInfo,
    js_runtime: Option<ProviderInfo>,
}

impl YtDlp {
    pub(super) fn discover() -> Result<Self, String> {
        let provider = provider::discover_ytdlp()
            .into_iter()
            .find(|info| info.compatible)
            .ok_or("A compatible system yt-dlp was not found; install it or configure the Arcade Box provider")?;
        let js_runtime = provider::discover_ytdlp_js_runtimes()
            .into_iter()
            .find(|info| info.compatible);
        Ok(Self {
            provider,
            js_runtime,
        })
    }

    /// Arguments every yt-dlp run shares: no user config or plugins, safe
    /// file names, a single video, and the JavaScript runtime when present.
    pub(super) fn base_args(&self, output_template: &str) -> Vec<OsString> {
        let mut args: Vec<OsString> = [
            "--ignore-config",
            "--no-plugin-dirs",
            "--no-cache-dir",
            "--no-colors",
            "--no-remote-components",
            "--no-playlist",
            "--restrict-filenames",
            "--no-mtime",
            "--no-warnings",
            "--no-js-runtimes",
            "--output",
            output_template,
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        if let Some(runtime) = &self.js_runtime {
            let name = if runtime.capability.ends_with(".deno") {
                "deno"
            } else {
                "node"
            };
            args.extend([
                "--js-runtimes".into(),
                format!("{name}:{}", runtime.executable_path.display()).into(),
            ]);
        }
        args
    }

    pub(super) fn spec(&self, args: Vec<OsString>, dir: &Path) -> ProcessSpec {
        ProcessSpec {
            executable: self.provider.executable_path.clone(),
            args,
            current_dir: Some(dir.to_path_buf()),
            timeout: PROCESS_TIMEOUT,
            output_limit: 2 * 1024 * 1024,
        }
    }

    fn run(
        &self,
        args: Vec<OsString>,
        dir: &Path,
        url: &str,
        cancelled: &AtomicBool,
    ) -> Result<(), String> {
        let output =
            process::run(&self.spec(args, dir), cancelled).map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(failure(&output.stderr, url))
        }
    }

    pub(super) fn runtime_warning(&self) -> Option<String> {
        self.js_runtime.is_none().then(|| {
            "No Deno or Node.js runtime was found. Some sites, including YouTube, may offer fewer formats.".into()
        })
    }
}

/// A readable yt-dlp error with the URL's query redacted.
pub(super) fn failure(stderr: &[u8], url: &str) -> String {
    let detail = network::process_error(stderr, "yt-dlp could not read this URL");
    format!(
        "{}\n\nArcade Box does not bypass DRM, paywalls, or private-content restrictions.",
        detail.replace(url, &network::redact_url(url))
    )
}

// ---------------------------------------------------------------------------
// Transcript
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
struct Cue {
    start: f64,
    end: f64,
    text: String,
}

fn parse_time(text: &str) -> Option<f64> {
    let text = text.trim().replace(',', ".");
    let parts = text.split(':').collect::<Vec<_>>();
    let seconds = parts.last()?.parse::<f64>().ok()?;
    let minutes = parts
        .get(parts.len().wrapping_sub(2))
        .and_then(|part| part.parse::<f64>().ok())
        .unwrap_or(0.0);
    let hours = if parts.len() == 3 {
        parts[0].parse::<f64>().ok()?
    } else {
        0.0
    };
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn strip_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_tag = false;
    for character in line.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(character),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
}

/// WebVTT cues with markup removed. Auto-generated captions repeat the
/// previous line in each cue ("rolling" captions); repeats are dropped.
fn vtt_cues(vtt: &str) -> Vec<Cue> {
    let mut cues: Vec<Cue> = Vec::new();
    let mut previous_lines: Vec<String> = Vec::new();
    let mut lines = vtt.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((start, rest)) = line.split_once("-->") else {
            continue;
        };
        let end = rest.split_whitespace().next().unwrap_or_default();
        let (Some(start), Some(end)) = (parse_time(start), parse_time(end)) else {
            continue;
        };
        let mut text_lines = Vec::new();
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                break;
            }
            let cleaned = strip_tags(lines.next().unwrap_or_default())
                .trim()
                .to_owned();
            if !cleaned.is_empty() {
                text_lines.push(cleaned);
            }
        }
        let fresh = text_lines
            .iter()
            .filter(|line| !previous_lines.contains(line))
            .cloned()
            .collect::<Vec<_>>();
        if !text_lines.is_empty() {
            previous_lines = text_lines;
        }
        if fresh.is_empty() || end - start < 0.02 {
            continue;
        }
        cues.push(Cue {
            start,
            end,
            text: fresh.join(" "),
        });
    }
    cues
}

fn srt(cues: &[Cue]) -> String {
    let stamp = |seconds: f64| {
        let millis = (seconds.max(0.0) * 1000.0).round() as u64;
        format!(
            "{:02}:{:02}:{:02},{:03}",
            millis / 3_600_000,
            millis / 60_000 % 60,
            millis / 1000 % 60,
            millis % 1000
        )
    };
    cues.iter()
        .enumerate()
        .map(|(index, cue)| {
            format!(
                "{}\n{} --> {}\n{}\n",
                index + 1,
                stamp(cue.start),
                stamp(cue.end),
                cue.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn plain_text(cues: &[Cue], timestamps: bool) -> String {
    if timestamps {
        return cues
            .iter()
            .map(|cue| {
                let seconds = cue.start as u64;
                format!("[{:02}:{:02}] {}", seconds / 60, seconds % 60, cue.text)
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    // Join into paragraphs, breaking on long pauses.
    let mut text = String::new();
    let mut last_end = 0.0;
    for cue in cues {
        if !text.is_empty() {
            text.push_str(if cue.start - last_end > 2.5 {
                "\n\n"
            } else {
                " "
            });
        }
        text.push_str(&cue.text);
        last_end = cue.end;
    }
    text
}

pub(super) fn transcript(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
    progress: Arc<dyn Fn(f64) + Send + Sync>,
) -> Result<ToolResult, String> {
    let url = url_input(request)?;
    let ytdlp = YtDlp::discover()?;
    let stage = stage_dir(runtime.artifact_staging_root())?;
    let language = option_str(request, "language", "en");
    if language.is_empty()
        || language.len() > 8
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || byte == b'-')
    {
        return Err("Choose a valid language".into());
    }
    let whisper = option_bool(request, "whisper", true);
    let mut args = ytdlp.base_args("%(title).90B.%(ext)s");
    // Exact tracks only: a pattern such as `en-.*` also matches dozens of
    // machine translations and trips YouTube's rate limit.
    args.extend(
        [
            "--skip-download",
            "--write-subs",
            "--write-auto-subs",
            "--sub-format",
            "vtt",
            "--sub-langs",
            &format!("{language},{language}-orig"),
            "--",
            &url,
        ]
        .map(OsString::from),
    );
    progress(0.1);
    let mut warnings = Vec::new();
    if let Err(error) = ytdlp.run(args, stage.path(), &url, cancelled) {
        if !whisper {
            return Err(error);
        }
        warnings.push(
            "The captions couldn't be downloaded, so the audio was transcribed instead.".to_owned(),
        );
    }
    let files = list_stage_outputs(stage.path())?;
    // Prefer the exact language, then the original-language track.
    let pick = |suffix: &str| files.iter().find(|(_, name)| name.ends_with(suffix));
    let caption = pick(&format!(".{language}.vtt"))
        .or_else(|| pick(&format!(".{language}-orig.vtt")))
        .or_else(|| files.iter().find(|(_, name)| name.ends_with(".vtt")));
    let (cues, title, source) = match caption {
        Some((path, name)) => {
            let vtt = fs::read_to_string(path).map_err(|error| error.to_string())?;
            let title = name.split('.').next().unwrap_or("transcript").to_owned();
            (vtt_cues(&vtt), title, "captions")
        }
        None if whisper => {
            // No captions: download the audio and transcribe it with Whisper.
            progress(0.2);
            let mut args = ytdlp.base_args("%(title).90B.%(ext)s");
            args.extend(["--format", "bestaudio/best", "--", &url].map(OsString::from));
            ytdlp.run(args, stage.path(), &url, cancelled)?;
            let (audio, name) = list_stage_outputs(stage.path())?
                .into_iter()
                .next()
                .ok_or("yt-dlp did not download any audio")?;
            check_cancelled(cancelled)?;
            let whisper_stage =
                tempfile::tempdir_in(stage.path()).map_err(|error| error.to_string())?;
            let report: Arc<dyn Fn(f64) + Send + Sync> = {
                let progress = progress.clone();
                Arc::new(move |fraction| progress(0.3 + fraction * 0.7))
            };
            let segments = crate::media::transcribe_to_segments(
                request,
                &audio,
                whisper_stage.path(),
                cancelled,
                report,
            )?;
            let cues = segments
                .into_iter()
                .map(|(start, end, text)| Cue { start, end, text })
                .collect();
            let title = name
                .rsplit_once('.')
                .map_or(name.as_str(), |(stem, _)| stem)
                .to_owned();
            (cues, title, "Whisper large-v3 (Groq)")
        }
        None => {
            return Err(format!(
                "This video has no `{language}` captions. Turn on Whisper fallback to transcribe the audio instead."
            ));
        }
    };
    if cues.is_empty() {
        return Err("The captions were empty".into());
    }
    let mut outputs = vec![ToolValue::text(
        plain_text(&cues, option_bool(request, "timestamps", false)),
        "text/plain",
    )];
    if option_bool(request, "subtitles", true) {
        let staged = stage.path().join("transcript.srt");
        fs::write(&staged, srt(&cues)).map_err(|error| error.to_string())?;
        outputs.push(publish_file(
            request,
            runtime,
            &staged,
            &output_name(request, &format!("{title}.srt"))?,
            cancelled,
        )?);
    }
    progress(1.0);
    let words = cues
        .iter()
        .map(|cue| cue.text.split_whitespace().count())
        .sum::<usize>();
    warnings.extend(ytdlp.runtime_warning());
    Ok(success(
        manifest,
        outputs,
        Some(format!("Transcript from {source}: {words} words")),
        warnings,
    ))
}

// ---------------------------------------------------------------------------
// Thumbnail
// ---------------------------------------------------------------------------

pub(super) fn thumbnail(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let url = url_input(request)?;
    let ytdlp = YtDlp::discover()?;
    let stage = stage_dir(runtime.artifact_staging_root())?;
    let format = match option_str(request, "format", "jpg") {
        format @ ("jpg" | "png" | "webp") => format,
        other => return Err(format!("Unknown format: {other}")),
    };
    let mut args = ytdlp.base_args("%(title).90B.%(ext)s");
    args.extend(
        [
            "--skip-download",
            "--write-thumbnail",
            "--convert-thumbnails",
            format,
            "--",
            &url,
        ]
        .map(OsString::from),
    );
    ytdlp.run(args, stage.path(), &url, cancelled)?;
    let (path, name) = list_stage_outputs(stage.path())?
        .into_iter()
        .find(|(_, name)| name.ends_with(&format!(".{format}")))
        .ok_or("This video has no thumbnail")?;
    let output = publish_file(
        request,
        runtime,
        &path,
        &output_name(request, &name)?,
        cancelled,
    )?;
    Ok(success(
        manifest,
        vec![output],
        Some(format!("Saved the thumbnail as {name}")),
        vec![],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolling_auto_captions_are_deduplicated() {
        let vtt = "WEBVTT\nKind: captions\n\n00:00:00.000 --> 00:00:02.000 align:start\nhello<00:00:00.500><c> world</c>\n\n00:00:02.000 --> 00:00:02.010\nhello world\n\n00:00:02.010 --> 00:00:04.000\nhello world\nthis is new\n\n00:00:09.000 --> 00:00:10.000\nlater &amp; on\n";
        let cues = vtt_cues(vtt);
        let texts = cues.iter().map(|cue| cue.text.as_str()).collect::<Vec<_>>();
        assert_eq!(texts, vec!["hello world", "this is new", "later & on"]);
        assert_eq!(
            plain_text(&cues, false),
            "hello world this is new\n\nlater & on"
        );
        assert!(srt(&cues).starts_with("1\n00:00:00,000 --> 00:00:02,000\nhello world\n"));
    }
}
