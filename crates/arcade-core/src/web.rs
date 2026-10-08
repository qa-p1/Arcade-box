//! First-party web and download actions. Remote inputs are explicit user
//! selections; requests and redirects are constrained to HTTP(S), output files
//! stay in private per-job staging until complete, and no shell is invoked.

use crate::{
    Arcade,
    artifacts::validate_portable_filename,
    network,
    process::{self, ProcessSpec},
    provider::ProviderInfo,
    tool_kit::{check_cancelled, option_bool, option_str, output_name, publish_file, success},
};
use arcade_contract::{ToolManifest, ToolRequest, ToolResult, ToolValue};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

mod page_images;
mod snapshot;

pub(crate) use snapshot::browser_available;
mod ytdlp;

const TEXT_LIMIT: u64 = 5 * 1024 * 1024;
const PROCESS_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);
const URL_FILE_NAME: &str = "download.bin";

pub fn execute_with_progress(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
    progress: Arc<dyn Fn(f64) + Send + Sync>,
) -> Result<ToolResult, String> {
    check_cancelled(cancelled)?;
    match manifest.id.as_str() {
        "arcade.web.downloader" => download_media(manifest, request, runtime, cancelled, progress),
        "arcade.web.file-downloader" => {
            download_file(manifest, request, runtime, cancelled, progress)
        }
        "arcade.web.markdown" => webpage_markdown(manifest, request, cancelled),
        "arcade.web.transcript" => {
            ytdlp::transcript(manifest, request, runtime, cancelled, progress)
        }
        "arcade.web.thumbnail" => ytdlp::thumbnail(manifest, request, runtime, cancelled),
        "arcade.web.images" => {
            page_images::download(manifest, request, runtime, cancelled, progress)
        }
        "arcade.web.snapshot" => snapshot::capture(manifest, request, runtime, cancelled, progress),
        _ => Err(format!("No web executor is registered for {}", manifest.id)),
    }
}

fn url_input(request: &ToolRequest) -> Result<String, String> {
    let [input] = request.inputs.as_slice() else {
        return Err("Enter one URL to continue".into());
    };
    network::validate_http_url(&input.value)
}

fn stage_dir(staging_root: &Path) -> Result<tempfile::TempDir, String> {
    tempfile::Builder::new()
        .prefix("job-")
        .tempdir_in(staging_root)
        .map_err(|error| format!("Could not create private download staging: {error}"))
}

fn download_file(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
    progress: Arc<dyn Fn(f64) + Send + Sync>,
) -> Result<ToolResult, String> {
    let url = url_input(request)?;
    let expected_sha256 = parse_expected_sha256(request)?;
    let curl = network::curl_provider()?;
    let stage = stage_dir(runtime.artifact_staging_root())?;
    let output = stage.path().join(URL_FILE_NAME);
    let headers_path = stage.path().join("response.headers");
    let mut total = probe_content_length(&curl, &url, cancelled).unwrap_or(0);
    let mut resume: Option<(String, u64)> = None;
    let mut info = None;
    let mut resume_attempts = 0u32;
    let mut transferred_bytes = 0u64;
    let mut resumed_file = false;
    progress(0.01);
    // Retry at most twice. Resume is attempted only after a failed response
    // with a strong entity tag and known complete length. The resumed response
    // is verified before any staged bytes can be published.
    for attempt in 0..3 {
        check_cancelled(cancelled)?;
        let resume_start = if resume.is_some() {
            fs::metadata(&output).ok().map(|metadata| metadata.len())
        } else {
            None
        };
        if resume.is_none() {
            let _ = fs::remove_file(&output);
            resumed_file = false;
        }
        let _ = fs::remove_file(&headers_path);
        let mut args = network::curl_common_args();
        args.extend([
            "--location".into(),
            "--max-redirs".into(),
            "10".into(),
            "--max-time".into(),
            PROCESS_TIMEOUT.as_secs().to_string().into(),
            "--fail".into(),
            "--max-filesize".into(),
            "214748364800".into(),
            "--header".into(),
            "Accept-Encoding: identity".into(),
        ]);
        if let Some((etag, _)) = &resume {
            args.extend([
                "--continue-at".into(),
                "-".into(),
                "--header".into(),
                format!("If-Range: {etag}").into(),
            ]);
            resume_attempts += 1;
        }
        args.extend([
            "--dump-header".into(),
            headers_path.as_os_str().to_owned(),
            "--output".into(),
            output.as_os_str().to_owned(),
            "--write-out".into(),
            "\nARCADE_FILE_META:%{url_effective}\t%{http_code}\t%{size_download}\t%{content_type}\n"
                .into(),
            "--".into(),
            url.clone().into(),
        ]);
        let current = run_with_file_progress(
            ProcessSpec {
                executable: curl.executable_path.clone(),
                args,
                current_dir: Some(stage.path().to_path_buf()),
                timeout: PROCESS_TIMEOUT,
                output_limit: 128 * 1024,
            },
            &output,
            total,
            cancelled,
            progress.clone(),
        )?;
        let header_text = fs::read_to_string(&headers_path).unwrap_or_default();
        let response_headers = parse_final_response_headers(&header_text);
        let curl_info = parse_curl_meta(
            &String::from_utf8_lossy(&current.stdout),
            "ARCADE_FILE_META",
        );
        if let Some(meta) = &curl_info {
            transferred_bytes = transferred_bytes.saturating_add(meta.bytes);
        }
        if let Some((etag, expected_total)) = &resume {
            let valid_resume = response_headers.status == Some(206)
                && response_headers.etag.as_deref() == Some(etag.as_str())
                && response_headers
                    .content_range
                    .as_deref()
                    .and_then(parse_content_range)
                    .is_some_and(|(start, end, reported_total)| {
                        Some(start) == resume_start
                            && start <= end
                            && end < reported_total
                            && reported_total == *expected_total
                    });
            if !valid_resume {
                // The origin changed or did not honor the range. Never use
                // appended bytes from an unverified representation.
                resume = None;
                resumed_file = false;
                let _ = fs::remove_file(&output);
                if attempt < 2 {
                    continue;
                }
                return Err("The server did not return a matching ranged response; the download was restarted but could not be verified".into());
            }
            resumed_file = true;
        }
        if current.status.success() {
            if let Some(total_from_headers) = response_headers.total_bytes {
                if total == 0 {
                    total = total_from_headers;
                }
            }
            if total > 0 {
                let size = fs::metadata(&output)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                if size != total {
                    // A successful HTTP exchange can still be incomplete if
                    // the server or intermediary reported inconsistent size.
                    resume = response_headers
                        .etag
                        .filter(|etag| is_strong_etag(etag))
                        .map(|etag| (etag, total));
                    if attempt < 2 && resume.is_some() && size > 0 && size < total {
                        continue;
                    }
                    return Err(format!(
                        "The server reported {total} bytes but the staged download contains {size}; the incomplete file was discarded"
                    ));
                }
            }
            info = curl_info;
            break;
        }
        let http_status = response_headers
            .status
            .or_else(|| curl_info.as_ref().map(|meta| meta.status));
        if http_status.is_some_and(|status| status >= 400) {
            let detail = network::process_error(&current.stderr, "Direct file download failed");
            return Err(detail.replace(&url, &network::redact_url(&url)));
        }
        if resume.is_some() {
            // The curl transfer itself failed, but keep the staged prefix only
            // when the original 200 response included a strong validator and
            // a known representation length.
        } else {
            let existing = fs::metadata(&output)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            let expected_total = (total > 0)
                .then_some(total)
                .or(response_headers.total_bytes);
            if existing > 0
                && let (Some(etag), Some(expected_total)) = (
                    response_headers.etag.filter(|tag| is_strong_etag(tag)),
                    expected_total,
                )
                && existing < expected_total
            {
                resume = Some((etag, expected_total));
                total = expected_total;
            }
        }
        if attempt == 2 {
            let detail = network::process_error(&current.stderr, "Direct file download failed");
            return Err(detail.replace(&url, &network::redact_url(&url)));
        }
        if resume.is_none() {
            let _ = fs::remove_file(&output);
            resumed_file = false;
        }
    }
    let info = info.ok_or("curl completed without download metadata")?;
    if !(200..300).contains(&info.status) {
        return Err(format!(
            "The server returned HTTP status {} instead of a successful file response",
            info.status
        ));
    }
    let actual = fs::symlink_metadata(&output)
        .map_err(|error| format!("Downloaded file is missing: {error}"))?;
    if !actual.is_file() || actual.file_type().is_symlink() || actual.len() == 0 {
        return Err("The server returned an empty or non-regular file".into());
    }
    let headers = fs::read_to_string(&headers_path).unwrap_or_default();
    let checksum = expected_sha256
        .as_deref()
        .map(|_| sha256_file(&output, cancelled))
        .transpose()?;
    if let (Some(expected), Some(actual)) = (expected_sha256.as_deref(), checksum.as_deref())
        && !actual.eq_ignore_ascii_case(expected)
    {
        return Err(format!(
            "SHA-256 did not match the expected value; the staged download was discarded (actual {actual})"
        ));
    }
    let name = output_name(
        request,
        &download_filename(&url, &headers, &info.content_type),
    );
    let name = name?;
    let artifact = publish_file(request, runtime, &output, &name, cancelled)?;
    progress(1.0);
    let mut result = success(
        manifest,
        vec![artifact],
        Some("Download finished".into()),
        vec![],
    );
    result.metadata.insert(
        "effectiveUrl".into(),
        json!(network::redact_url(&info.effective_url)),
    );
    result
        .metadata
        .insert("httpStatus".into(), json!(info.status));
    result.metadata.insert("bytes".into(), json!(actual.len()));
    result
        .metadata
        .insert("transferredBytes".into(), json!(transferred_bytes));
    result
        .metadata
        .insert("resumeAttempts".into(), json!(resume_attempts));
    if let Some(checksum) = checksum {
        result.metadata.insert("sha256".into(), json!(checksum));
    }
    result
        .metadata
        .insert("contentType".into(), json!(info.content_type));
    if let Some(expected) = expected_sha256 {
        result
            .metadata
            .insert("checksumVerified".into(), json!(true));
        result.metadata.insert(
            "expectedSha256".into(),
            json!(expected.to_ascii_lowercase()),
        );
    }
    if resumed_file {
        result.metadata.insert("resumed".into(), json!(true));
    }
    Ok(result)
}

fn parse_expected_sha256(request: &ToolRequest) -> Result<Option<String>, String> {
    let Some(value) = request.options.get("expectedSha256") else {
        return Ok(None);
    };
    let Some(value) = value.as_str() else {
        return Err("Expected SHA-256 must be a 64-character hexadecimal string".into());
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Expected SHA-256 must be a 64-character hexadecimal string".into());
    }
    Ok(Some(value.to_ascii_lowercase()))
}

fn sha256_file(path: &Path, cancelled: &AtomicBool) -> Result<String, String> {
    let mut file =
        fs::File::open(path).map_err(|error| format!("Could not verify download: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        check_cancelled(cancelled)?;
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("Could not read download for checksum: {error}"))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Default)]
struct FinalResponseHeaders {
    status: Option<u16>,
    etag: Option<String>,
    total_bytes: Option<u64>,
    content_range: Option<String>,
}

fn parse_final_response_headers(raw: &str) -> FinalResponseHeaders {
    let mut final_block = None;
    for block in raw.split("\r\n\r\n") {
        let mut lines = block.lines();
        let Some(status_line) = lines.next() else {
            continue;
        };
        if !status_line.starts_with("HTTP/") {
            continue;
        }
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse().ok());
        let mut headers = FinalResponseHeaders {
            status,
            ..FinalResponseHeaders::default()
        };
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            if name.eq_ignore_ascii_case("etag") {
                headers.etag = Some(value.to_owned());
            } else if name.eq_ignore_ascii_case("content-length") {
                headers.total_bytes = value.parse().ok();
            } else if name.eq_ignore_ascii_case("content-range") {
                headers.content_range = Some(value.to_owned());
                if let Some((_, _, total)) = parse_content_range(value) {
                    headers.total_bytes = Some(total);
                }
            }
        }
        final_block = Some(headers);
    }
    final_block.unwrap_or_default()
}

fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let range = value.trim().strip_prefix("bytes ")?;
    let (span, total) = range.split_once('/')?;
    let total = total.parse::<u64>().ok()?;
    let (start, end) = span.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total))
}

fn is_strong_etag(value: &str) -> bool {
    let bytes = value.trim().as_bytes();
    bytes.len() >= 2
        && bytes[0] == b'"'
        && bytes[bytes.len() - 1] == b'"'
        && bytes[1..bytes.len() - 1]
            .iter()
            .all(|byte| *byte >= 0x21 && *byte != 0x22 && *byte != 0x7f)
}

fn probe_content_length(provider: &ProviderInfo, url: &str, cancelled: &AtomicBool) -> Option<u64> {
    let mut args = network::curl_common_args();
    args.extend([
        "--head".into(),
        "--location".into(),
        "--max-redirs".into(),
        "10".into(),
        "--header".into(),
        "Accept-Encoding: identity".into(),
        "--".into(),
        url.into(),
    ]);
    let output = process::run(
        &ProcessSpec {
            executable: provider.executable_path.clone(),
            args,
            current_dir: None,
            timeout: Duration::from_secs(20),
            output_limit: 256 * 1024,
        },
        cancelled,
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(key, _)| key.trim().eq_ignore_ascii_case("content-length"))?
                .1
                .trim()
                .parse()
                .ok()
        })
}

fn run_with_file_progress(
    spec: ProcessSpec,
    path: &Path,
    total: u64,
    cancelled: &AtomicBool,
    progress: Arc<dyn Fn(f64) + Send + Sync>,
) -> Result<process::ProcessOutput, String> {
    thread::scope(|scope| {
        let handle = scope.spawn(|| process::run(&spec, cancelled));
        let mut last = -1.0f64;
        loop {
            if handle.is_finished() {
                return handle
                    .join()
                    .map_err(|_| "Download worker failed".to_owned())?
                    .map_err(|error| error.to_string());
            }
            if cancelled.load(Ordering::Acquire) {
                // The process runner observes the same cancellation flag and
                // terminates the complete provider process tree.
            }
            if let Ok(metadata) = fs::metadata(path) {
                let fraction = if total > 0 {
                    (0.03 + 0.94 * (metadata.len().min(total) as f64 / total as f64)).min(0.97)
                } else if metadata.len() > 0 {
                    0.1
                } else {
                    0.02
                };
                if fraction > last {
                    progress(fraction);
                    last = fraction;
                }
            }
            thread::sleep(Duration::from_millis(250));
        }
    })
}

struct CurlMeta {
    effective_url: String,
    status: u16,
    bytes: u64,
    content_type: String,
}

fn parse_curl_meta(stdout: &str, marker: &str) -> Option<CurlMeta> {
    let (_, metadata) = stdout.rsplit_once(&format!("\n{marker}:"))?;
    let fields = metadata.trim().split('\t').collect::<Vec<_>>();
    if fields.len() != 4 {
        return None;
    }
    Some(CurlMeta {
        effective_url: fields[0].to_owned(),
        status: fields[1].parse().ok()?,
        bytes: fields[2].parse().ok()?,
        content_type: fields[3].to_owned(),
    })
}

fn download_filename(url: &str, headers: &str, content_type: &str) -> String {
    if let Some(value) = headers.lines().rev().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if !key.trim().eq_ignore_ascii_case("content-disposition") {
            return None;
        }
        parse_content_disposition(value.trim())
    }) {
        return sanitize_filename(&value, content_type);
    }
    let path = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    let basename = path.rsplit('/').next().unwrap_or_default();
    sanitize_filename(&percent_decode(basename), content_type)
}

fn parse_content_disposition(value: &str) -> Option<String> {
    for parameter in value.split(';').skip(1) {
        let (key, value) = parameter.trim().split_once('=')?;
        if key.eq_ignore_ascii_case("filename*") {
            let encoded = value.trim_matches('"');
            let bytes = encoded
                .split_once("''")
                .map(|(_, data)| data)
                .unwrap_or(encoded);
            return Some(percent_decode(bytes));
        }
        if key.eq_ignore_ascii_case("filename") {
            return Some(value.trim().trim_matches('"').replace("\\\"", "\""));
        }
    }
    None
}

fn sanitize_filename(raw: &str, content_type: &str) -> String {
    let mut name = raw
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*'
                )
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_owned();
    if name.is_empty() || validate_portable_filename(&name).is_err() {
        name = "download".into();
    }
    if Path::new(&name).extension().is_none() {
        let extension = content_type_extension(content_type);
        name.push_str(extension);
    }
    if validate_portable_filename(&name).is_err() {
        "download.bin".into()
    } else {
        name
    }
}

fn content_type_extension(content_type: &str) -> &'static str {
    match content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "application/pdf" => ".pdf",
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/webp" => ".webp",
        "video/mp4" => ".mp4",
        "video/webm" => ".webm",
        "audio/mpeg" => ".mp3",
        "audio/wav" | "audio/x-wav" => ".wav",
        "text/html" => ".html",
        "text/plain" => ".txt",
        "application/zip" => ".zip",
        _ => ".bin",
    }
}

fn percent_decode(value: &str) -> String {
    let mut bytes = Vec::with_capacity(value.len());
    let input = value.as_bytes();
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' && index + 2 < input.len() {
            if let (Some(hi), Some(lo)) = (hex_value(input[index + 1]), hex_value(input[index + 2]))
            {
                bytes.push(hi * 16 + lo);
                index += 3;
                continue;
            }
        }
        bytes.push(input[index]);
        index += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn download_media(
    manifest: &ToolManifest,
    request: &ToolRequest,
    runtime: &Arcade,
    cancelled: &AtomicBool,
    progress: Arc<dyn Fn(f64) + Send + Sync>,
) -> Result<ToolResult, String> {
    let url = url_input(request)?;
    let ytdlp = ytdlp::YtDlp::discover()?;
    let stage = stage_dir(runtime.artifact_staging_root())?;
    let audio = option_str(request, "media", "video") == "audio";
    let quality = option_str(request, "quality", "best");
    let output_format = option_str(
        request,
        if audio { "audioFormat" } else { "videoFormat" },
        option_str(request, "format", "original"),
    );
    let template_stem = output_template_stem(request)?;
    let mut args = ytdlp.base_args(&format!("{template_stem}.%(ext)s"));
    args.extend(
        [
            "--no-overwrites",
            "--no-continue",
            "--abort-on-unavailable-fragments",
            "--newline",
            "--progress",
        ]
        .map(OsString::from),
    );
    if audio {
        args.extend([
            OsString::from("--extract-audio"),
            OsString::from("--audio-format"),
            OsString::from(audio_format(output_format)?),
        ]);
        let audio_quality = option_str(request, "audioQuality", "5");
        if let Ok(value) = audio_quality.parse::<u8>() {
            if value > 10 {
                return Err("Audio quality must be from 0 to 10".into());
            }
            args.extend([
                OsString::from("--audio-quality"),
                OsString::from(value.to_string()),
            ]);
        } else if audio_quality != "best" {
            return Err("Choose a valid audio quality".into());
        }
    } else {
        args.extend([
            OsString::from("--format"),
            OsString::from(video_format_selector(quality)?),
        ]);
        match output_format {
            "original" | "" => {}
            "mp4" | "mkv" | "webm" => args.extend([
                OsString::from("--merge-output-format"),
                OsString::from(output_format),
            ]),
            _ => return Err("For video, choose Original, MP4, MKV, or WebM".into()),
        }
    }
    if option_bool(request, "subtitles", false) {
        args.extend([
            OsString::from("--write-subs"),
            OsString::from("--write-auto-subs"),
            OsString::from("--sub-langs"),
            OsString::from("all"),
        ]);
    }
    if option_bool(request, "thumbnail", false) {
        args.push(OsString::from("--write-thumbnail"));
    }
    if option_bool(request, "metadata", true) {
        args.push(OsString::from("--embed-metadata"));
    }
    args.extend([OsString::from("--"), OsString::from(&url)]);
    progress(0.02);
    let output = run_with_directory_progress(
        ytdlp.spec(args, stage.path()),
        stage.path(),
        cancelled,
        progress.clone(),
    )?;
    if !output.status.success() {
        return Err(ytdlp::failure(&output.stderr, &url));
    }
    let files = list_stage_outputs(stage.path())?;
    if files.is_empty() {
        return Err("yt-dlp completed without producing an output file".into());
    }
    let main = files
        .iter()
        .find(|(path, _)| {
            let ext = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            matches!(
                ext.as_str(),
                "mp4"
                    | "mkv"
                    | "webm"
                    | "mov"
                    | "m4v"
                    | "mp3"
                    | "m4a"
                    | "opus"
                    | "flac"
                    | "wav"
                    | "ogg"
            )
        })
        .map(|(path, _)| path.clone())
        .unwrap_or_else(|| files[0].0.clone());
    let mut artifacts = Vec::with_capacity(files.len());
    for (path, name) in &files {
        check_cancelled(cancelled)?;
        let final_name = if path == &main {
            output_name(request, name)?
        } else {
            name.clone()
        };
        artifacts.push(publish_file(
            request,
            runtime,
            path,
            &final_name,
            cancelled,
        )?);
    }
    progress(1.0);
    let mut result = success(
        manifest,
        artifacts,
        Some("Media download finished".into()),
        vec![],
    );
    result.metadata.insert("provider".into(), json!({"source": ytdlp.provider.source, "path": ytdlp.provider.executable_path, "version": ytdlp.provider.version}));
    result.warnings.extend(ytdlp.runtime_warning());
    result.warnings.push("Authenticated browser cookies are not read or exposed. Download only content you own or are authorized to save.".into());
    Ok(result)
}

fn output_template_stem(request: &ToolRequest) -> Result<String, String> {
    // yt-dlp fills in the video title (at most 90 bytes).
    let fallback = "%(title).90B";
    let Some(name) = request.options.get("outputName").and_then(Value::as_str) else {
        return Ok(fallback.into());
    };
    if name.trim().is_empty() {
        return Ok(fallback.into());
    }
    validate_portable_filename(name.trim())?;
    let stem = Path::new(name.trim())
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(name.trim());
    if stem.is_empty() || stem.len() > 100 {
        return Err(
            "Output name must have a non-empty basename no longer than 100 characters".into(),
        );
    }
    Ok(stem.to_owned())
}

fn video_format_selector(quality: &str) -> Result<&'static str, String> {
    match quality {
        "best" => Ok("bestvideo*+bestaudio/best"),
        "2160" => Ok("bestvideo[height<=2160]+bestaudio/best[height<=2160]"),
        "1440" => Ok("bestvideo[height<=1440]+bestaudio/best[height<=1440]"),
        "1080" => Ok("bestvideo[height<=1080]+bestaudio/best[height<=1080]"),
        "720" => Ok("bestvideo[height<=720]+bestaudio/best[height<=720]"),
        "480" => Ok("bestvideo[height<=480]+bestaudio/best[height<=480]"),
        "360" => Ok("bestvideo[height<=360]+bestaudio/best[height<=360]"),
        _ => Err("Choose a supported video quality".into()),
    }
}

fn audio_format(format: &str) -> Result<&'static str, String> {
    match format {
        "original" | "best" | "" => Ok("best"),
        "mp3" => Ok("mp3"),
        "m4a" => Ok("m4a"),
        "flac" => Ok("flac"),
        "opus" => Ok("opus"),
        "wav" => Ok("wav"),
        _ => Err("Choose Original, MP3, M4A, FLAC, Opus, or WAV for audio".into()),
    }
}

fn run_with_directory_progress(
    spec: ProcessSpec,
    directory: &Path,
    cancelled: &AtomicBool,
    progress: Arc<dyn Fn(f64) + Send + Sync>,
) -> Result<process::ProcessOutput, String> {
    thread::scope(|scope| {
        let handle = scope.spawn(|| process::run(&spec, cancelled));
        let mut previous = 0u64;
        loop {
            if handle.is_finished() {
                return handle
                    .join()
                    .map_err(|_| "Download worker failed".to_owned())?
                    .map_err(|error| error.to_string());
            }
            let current = fs::read_dir(directory)
                .ok()
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter_map(|entry| entry.metadata().ok())
                .filter(|meta| meta.is_file())
                .map(|meta| meta.len())
                .sum::<u64>();
            if current > previous {
                // yt-dlp's progress template is not guaranteed to flush from
                // a redirected non-TTY; disk growth gives a conservative
                // active signal without inventing an ETA or total percentage.
                progress(0.05);
                previous = current;
            }
            thread::sleep(Duration::from_millis(350));
        }
    })
}

fn list_stage_outputs(directory: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let mut outputs = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("Provider created a symlink inside its output directory".into());
        }
        if metadata.is_file() {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "Provider created a filename that cannot be represented safely")?;
            validate_portable_filename(&name)
                .map_err(|_| "Provider created an unsafe output filename")?;
            outputs.push((entry.path(), name));
        } else if metadata.is_dir() {
            return Err(
                "Provider wrote outside the flat per-job output set; refusing nested output".into(),
            );
        }
        if outputs.len() > 64 {
            return Err("Provider created too many output files".into());
        }
    }
    outputs.sort_by(|left, right| left.1.cmp(&right.1));
    Ok(outputs)
}

struct FetchedPage {
    body: Vec<u8>,
    url: String,
}

fn fetch_page(
    url: &str,
    max_bytes: usize,
    staging: &Path,
    cancelled: &AtomicBool,
) -> Result<FetchedPage, String> {
    let curl = network::curl_provider()?;
    let mut args = network::curl_common_args();
    args.extend([
        "--location".into(),
        "--max-redirs".into(),
        "10".into(),
        "--fail".into(),
        "--max-filesize".into(),
        OsString::from(max_bytes.to_string()),
        "--output".into(),
        "-".into(),
        "--write-out".into(),
        "\nARCADE_WEB_META:%{url_effective}\t%{content_type}\t%{http_code}\n".into(),
        "--".into(),
        url.into(),
    ]);
    let output = process::run(
        &ProcessSpec {
            executable: curl.executable_path,
            args,
            current_dir: Some(staging.to_path_buf()),
            timeout: Duration::from_secs(45),
            output_limit: max_bytes + 64 * 1024,
        },
        cancelled,
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let detail = network::process_error(&output.stderr, "Could not fetch this webpage");
        return Err(detail.replace(url, &network::redact_url(url)));
    }
    let marker = b"\nARCADE_WEB_META:";
    let split_at = output
        .stdout
        .windows(marker.len())
        .rposition(|window| window == marker)
        .ok_or("curl returned no webpage metadata")?;
    let mut body = output.stdout[..split_at].to_vec();
    // Remove one optional CRLF left between body data and the write-out line.
    if body.ends_with(b"\r\n") {
        body.truncate(body.len() - 2);
    } else if body.ends_with(b"\n") {
        body.pop();
    }
    if body.len() > max_bytes {
        return Err(format!("Webpage exceeds the {max_bytes} byte text limit"));
    }
    let metadata = String::from_utf8_lossy(&output.stdout[split_at + marker.len()..]);
    let fields = metadata.trim().split('\t').collect::<Vec<_>>();
    if fields.len() != 3 {
        return Err("curl returned incomplete webpage metadata".into());
    }
    if fields[2].parse::<u16>().unwrap_or(0) >= 400 {
        return Err(format!("Web server returned HTTP {}", fields[2]));
    }
    if !fields[1].is_empty() && !fields[1].to_ascii_lowercase().contains("text/html") {
        return Err(format!(
            "This URL returned {} rather than an HTML page",
            fields[1]
        ));
    }
    Ok(FetchedPage {
        body,
        url: fields[0].to_owned(),
    })
}

fn webpage_markdown(
    manifest: &ToolManifest,
    request: &ToolRequest,
    cancelled: &AtomicBool,
) -> Result<ToolResult, String> {
    let url = url_input(request)?;
    let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
    let page = fetch_page(&url, TEXT_LIMIT as usize, temp.path(), cancelled)?;
    let html = String::from_utf8_lossy(&page.body);
    let markdown = html_to_markdown(&html, &page.url);
    if markdown.trim().is_empty() {
        return Err("No readable text was found in the static webpage source".into());
    }
    let mut result = success(
        manifest,
        vec![ToolValue::text(markdown, "text/markdown")],
        Some("Readable text extracted".into()),
        vec![],
    );
    result
        .metadata
        .insert("sourceUrl".into(), json!(network::redact_url(&page.url)));
    result
        .warnings
        .push("The extractor works on static HTML and does not execute page JavaScript.".into());
    Ok(result)
}

fn html_to_markdown(html: &str, base_url: &str) -> String {
    let mut output = String::with_capacity(html.len().min(1024 * 1024));
    let mut index = 0;
    let mut skip_tag: Option<String> = None;
    let mut link_stack: Vec<Option<String>> = Vec::new();
    while index < html.len() {
        if let Some(open_rel) = html[index..].find('<') {
            let open = index + open_rel;
            if skip_tag.is_none() && open > index {
                append_text(&mut output, &html[index..open]);
            }
            let Some(close_rel) = html[open..].find('>') else {
                break;
            };
            let close = open + close_rel;
            let raw = html[open + 1..close].trim();
            if raw.starts_with("!--") {
                if let Some(end_rel) = html[close + 1..].find("-->") {
                    index = close + 1 + end_rel + 3;
                    continue;
                }
                break;
            }
            let Some((tag, closing, attributes)) = parse_tag(raw) else {
                index = close + 1;
                continue;
            };
            if skip_tag.is_some() {
                if closing && skip_tag.as_deref() == Some(tag.as_str()) {
                    skip_tag = None;
                }
                index = close + 1;
                continue;
            }
            if matches!(
                tag.as_str(),
                "script" | "style" | "noscript" | "svg" | "template"
            ) && !closing
            {
                skip_tag = Some(tag);
                index = close + 1;
                continue;
            }
            if closing {
                match tag.as_str() {
                    "a" => {
                        if link_stack.pop().flatten().is_some() {
                            output.push(')');
                        }
                    }
                    "h1" | "h2" | "h3" | "h4" | "p" | "div" | "section" | "article" | "header"
                    | "footer" | "blockquote" | "pre" => ensure_blank_line(&mut output),
                    "li" => ensure_line_break(&mut output),
                    _ => {}
                }
            } else {
                match tag.as_str() {
                    "h1" | "h2" | "h3" | "h4" => {
                        ensure_blank_line(&mut output);
                        let count = tag[1..].parse::<usize>().unwrap_or(2).min(4);
                        output.push_str(&"#".repeat(count));
                        output.push(' ');
                    }
                    "p" | "div" | "section" | "article" | "header" | "footer" | "blockquote" => {
                        ensure_blank_line(&mut output)
                    }
                    "br" | "hr" => ensure_line_break(&mut output),
                    "li" => {
                        ensure_line_break(&mut output);
                        output.push_str("- ");
                    }
                    "pre" => {
                        ensure_blank_line(&mut output);
                        output.push_str("```\n");
                    }
                    "code" => output.push('`'),
                    "a" => {
                        let url = attributes
                            .get("href")
                            .and_then(|href| network::resolve_url(base_url, href));
                        link_stack.push(url.clone());
                        if let Some(url) = url {
                            output.push('[');
                            output.push_str(&network::redact_url(&url));
                            output.push_str("](");
                        }
                    }
                    "img" => {
                        if let Some(alt) = attributes.get("alt") {
                            output.push_str("![");
                            output.push_str(&markdown_escape(alt));
                            output.push(']');
                            if let Some(src) = attributes
                                .get("src")
                                .and_then(|src| network::resolve_url(base_url, src))
                            {
                                output.push('(');
                                output.push_str(&network::redact_url(&src));
                                output.push(')');
                            }
                        }
                    }
                    _ => {}
                }
            }
            index = close + 1;
        } else {
            if skip_tag.is_none() {
                append_text(&mut output, &html[index..]);
            }
            break;
        }
    }
    let normalized = output.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = normalized
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    bounded_text(&normalized, 1024 * 1024).trim().to_owned()
}

fn parse_tag(raw: &str) -> Option<(String, bool, std::collections::HashMap<String, String>)> {
    let raw = raw.trim();
    let closing = raw.starts_with('/');
    let raw = raw.trim_start_matches('/').trim_start();
    let tag_end = raw
        .find(|ch: char| ch.is_whitespace() || ch == '/')
        .unwrap_or(raw.len());
    if tag_end == 0 {
        return None;
    }
    let tag = raw[..tag_end].to_ascii_lowercase();
    if !tag
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b':')
    {
        return None;
    }
    let attributes = if closing {
        Default::default()
    } else {
        parse_attributes(&raw[tag_end..])
    };
    Some((tag, closing, attributes))
}

fn parse_attributes(mut raw: &str) -> std::collections::HashMap<String, String> {
    let mut attributes = std::collections::HashMap::new();
    while !raw.trim_start().is_empty() {
        raw = raw.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '/');
        if raw.is_empty() {
            break;
        }
        let key_end = raw
            .find(|ch: char| ch.is_whitespace() || ch == '=' || ch == '/')
            .unwrap_or(raw.len());
        if key_end == 0 {
            raw = &raw[1..];
            continue;
        }
        let key = raw[..key_end].to_ascii_lowercase();
        raw = &raw[key_end..];
        raw = raw.trim_start();
        let mut value = String::new();
        if let Some(rest) = raw.strip_prefix('=') {
            raw = rest.trim_start();
            if let Some(quote) = raw.chars().next().filter(|ch| *ch == '\'' || *ch == '"') {
                raw = &raw[quote.len_utf8()..];
                if let Some(end) = raw.find(quote) {
                    value = decode_entities(&raw[..end]);
                    raw = &raw[end + quote.len_utf8()..];
                } else {
                    value = decode_entities(raw);
                    raw = "";
                }
            } else {
                let end = raw.find(char::is_whitespace).unwrap_or(raw.len());
                value = decode_entities(&raw[..end]);
                raw = &raw[end..];
            }
        }
        if attributes.len() < 64 {
            attributes.entry(key).or_insert(value);
        }
    }
    attributes
}

fn append_text(output: &mut String, text: &str) {
    let decoded = decode_entities(text);
    for ch in decoded.chars() {
        match ch {
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '\r' => output.push('\n'),
            _ => output.push(ch),
        }
    }
}

fn markdown_escape(text: &str) -> String {
    text.replace('[', "\\[")
        .replace(']', "\\]")
        .replace('\n', " ")
}

fn ensure_line_break(output: &mut String) {
    while output.ends_with(' ') {
        output.pop();
    }
    if !output.ends_with('\n') {
        output.push('\n');
    }
}
fn ensure_blank_line(output: &mut String) {
    ensure_line_break(output);
    if !output.ends_with("\n\n") {
        output.push('\n');
    }
}

fn decode_entities(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let remaining = &text[index..];
        if let Some(end) = remaining
            .find(';')
            .filter(|end| remaining.starts_with('&') && (2..=12).contains(end))
        {
            let entity = &remaining[1..end];
            let decoded = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    u32::from_str_radix(&entity[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                }
                _ if entity.starts_with('#') => {
                    entity[1..].parse::<u32>().ok().and_then(char::from_u32)
                }
                _ => None,
            };
            if let Some(ch) = decoded {
                output.push(ch);
                index += end + 1;
                continue;
            }
        }
        let ch = remaining.chars().next().unwrap();
        output.push(ch);
        index += ch.len_utf8();
    }
    output
}

fn bounded_text(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_and_url_filenames_are_sanitized() {
        assert_eq!(
            parse_content_disposition("attachment; filename*=UTF-8''report%20one.pdf").as_deref(),
            Some("report one.pdf")
        );
        assert_eq!(sanitize_filename("../CON", "application/pdf"), "_CON.pdf");
        assert_eq!(
            download_filename("https://example.com/file", "", "application/pdf"),
            "file.pdf"
        );
    }

    #[test]
    fn downloader_resume_requires_strong_etag_and_exact_range_metadata() {
        assert!(is_strong_etag("\"abc\""));
        assert!(!is_strong_etag("W/\"abc\""));
        assert!(!is_strong_etag("abc"));
        assert_eq!(parse_content_range("bytes 12-19/40"), Some((12, 19, 40)));
        assert_eq!(parse_content_range("bytes 12-19/*"), None);
        let headers = parse_final_response_headers(
            "HTTP/1.1 200 OK\r\nContent-Length: 40\r\nETag: \"v1\"\r\n\r\n",
        );
        assert_eq!(headers.status, Some(200));
        assert_eq!(headers.total_bytes, Some(40));
        assert_eq!(headers.etag.as_deref(), Some("\"v1\""));
    }

    #[test]
    fn expected_sha256_must_be_a_hex_digest() {
        let request = ToolRequest {
            tool_id: "arcade.web.file-downloader".into(),
            inputs: Vec::new(),
            options: json!({ "expectedSha256": "a".repeat(64) }),
        };
        assert_eq!(
            parse_expected_sha256(&request).unwrap(),
            Some("a".repeat(64))
        );
        let invalid = ToolRequest {
            options: json!({ "expectedSha256": "not-a-digest" }),
            ..request
        };
        assert!(parse_expected_sha256(&invalid).is_err());
    }

    #[test]
    fn html_extractor_skips_active_content_and_rejects_script_urls() {
        let markdown = html_to_markdown(
            "<h1>Hello</h1><script>secret()</script><p>Read &amp; learn</p><a href=\"javascript:alert(1)\">unsafe</a>",
            "https://example.com/page",
        );
        assert!(markdown.contains("# Hello"));
        assert!(markdown.contains("Read & learn"));
        assert!(!markdown.contains("secret()"));
        assert!(!markdown.contains("javascript:"));
    }
}
