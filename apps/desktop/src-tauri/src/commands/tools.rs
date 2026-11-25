//! The tool catalog, search, jobs, history, favourites, aliases, preferences and context actions the frontend runs.

use crate::*;

#[tauri::command]
pub(crate) fn list_tools(runtime: tauri::State<'_, Arc<Arcade>>) -> Vec<ToolManifest> {
    runtime
        .list_tools()
        .into_iter()
        .filter(desktop_tool_supported)
        .collect()
}

pub(crate) fn desktop_tool_supported(tool: &ToolManifest) -> bool {
    // macOS has no supported API to pin another application's window.
    !(cfg!(target_os = "macos") && tool.id == "arcade.system.window-pin")
}

#[tauri::command]
pub(crate) fn search_tools(
    query: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Vec<ToolManifest> {
    runtime
        .search_tools(&query)
        .into_iter()
        .filter(desktop_tool_supported)
        .collect()
}

#[tauri::command]
pub(crate) fn get_history(
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Vec<HistoryEntry>, String> {
    runtime
        .storage()
        .recent_history(50)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn list_favorites(
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Vec<String>, String> {
    runtime
        .storage()
        .favorites()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn set_favorite(
    tool_id: String,
    favorite: bool,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    if !runtime.list_tools().iter().any(|tool| tool.id == tool_id) {
        return Err("Unknown tool ID".into());
    }
    runtime
        .storage()
        .set_favorite(&tool_id, favorite)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn set_alias(
    alias: String,
    tool_id: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    if !runtime.list_tools().iter().any(|tool| tool.id == tool_id) {
        return Err("Unknown tool ID".into());
    }
    if alias.trim().is_empty() || alias.len() > 80 {
        return Err("Alias must contain 1 to 80 characters".into());
    }
    runtime
        .storage()
        .set_alias(&alias, &tool_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn get_preference(
    key: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Option<String>, String> {
    if !matches!(key.as_str(), "onboarding_complete" | "theme") {
        return Err("Unknown preference".into());
    }
    runtime
        .storage()
        .setting(&key)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn set_preference(
    key: String,
    value: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    let valid = match key.as_str() {
        "onboarding_complete" => matches!(value.as_str(), "true" | "false"),
        "theme" => matches!(value.as_str(), "system" | "light" | "dark"),
        _ => false,
    };
    if !valid {
        return Err("Unsupported preference value".into());
    }
    runtime
        .storage()
        .set_setting(&key, &value)
        .map_err(|error| error.to_string())
}

pub(crate) fn validate_frontend_request(
    request: &ToolRequest,
    runtime: &Arcade,
) -> Result<(), String> {
    if request
        .inputs
        .iter()
        .any(|input| input.kind == ValueKind::File)
    {
        return Err("Raw file paths are not accepted; use Arcade Box's scoped file picker".into());
    }
    if request
        .inputs
        .iter()
        .map(|input| input.value.len())
        .sum::<usize>()
        > 16 * 1024 * 1024
    {
        return Err("Text input exceeds the 16 MB interactive limit; use a file workflow".into());
    }
    let tool = runtime
        .list_tools()
        .into_iter()
        .find(|tool| tool.id == request.tool_id)
        .ok_or("Unknown tool ID")?;
    if tool.status == ImplementationStatus::Planned {
        return Err("This tool is not implemented yet".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn list_jobs(jobs: tauri::State<'_, JobManager>) -> Vec<JobSnapshot> {
    jobs.list()
}

#[tauri::command]
pub(crate) fn start_job(
    request: ToolRequest,
    runtime: tauri::State<'_, Arc<Arcade>>,
    jobs: tauri::State<'_, JobManager>,
) -> Result<JobSnapshot, String> {
    validate_frontend_request(&request, &runtime)?;
    jobs.submit(request).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn cancel_job(
    job_id: String,
    jobs: tauri::State<'_, JobManager>,
) -> Result<JobSnapshot, String> {
    jobs.cancel(&job_id).map_err(|error| error.to_string())
}

#[tauri::command(async)]
pub(crate) fn run_context_action(
    tool_id: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<ToolResult, String> {
    if !matches!(
        tool_id.as_str(),
        "arcade.text.structured"
            | "arcade.text.case"
            | "arcade.text.clean"
            | "arcade.barcode.qr-generate"
    ) {
        return Err("This clipboard action is not available".into());
    }
    let text = app
        .clipboard()
        .read_text()
        .map_err(|error| error.to_string())?;
    if text.len() > 16 * 1024 * 1024 {
        return Err("Clipboard text exceeds the interactive limit".into());
    }
    let options = if tool_id == "arcade.text.structured" {
        serde_json::json!({"to": "same"})
    } else {
        serde_json::Value::Null
    };
    runtime
        .run_tool(ToolRequest {
            tool_id,
            inputs: vec![ToolValue::text(text, "text/plain")],
            options,
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn detect_context(app: tauri::AppHandle) -> Vec<ContextSuggestion> {
    let Ok(text) = app.clipboard().read_text() else {
        return vec![];
    };
    if text.len() > 16 * 1024 * 1024 {
        return vec![];
    }
    context_suggestions(&text)
}

fn suggestion(tool_id: &str, reason: &str, input: Option<&str>) -> ContextSuggestion {
    ContextSuggestion {
        tool_id: tool_id.into(),
        reason: reason.into(),
        input: input.map(str::to_owned),
    }
}

/// Sites whose pages yt-dlp downloads as video or audio.
const MEDIA_HOSTS: &[&str] = &[
    "youtube.com",
    "youtu.be",
    "vimeo.com",
    "dailymotion.com",
    "twitch.tv",
    "tiktok.com",
    "instagram.com",
    "facebook.com",
    "fb.watch",
    "x.com",
    "twitter.com",
    "reddit.com",
    "soundcloud.com",
    "bandcamp.com",
    "bilibili.com",
    "streamable.com",
    "rumble.com",
];

/// Hosts whose videos have captions that the transcript tool can read.
const TRANSCRIPT_HOSTS: &[&str] = &["youtube.com", "youtu.be", "vimeo.com", "dailymotion.com"];

const MEDIA_EXTENSIONS: &[&str] = &[
    "mp4", "webm", "mkv", "mov", "m4v", "m3u8", "mpd", "mp3", "m4a", "ogg", "opus", "wav", "flac",
];

const FILE_EXTENSIONS: &[&str] = &[
    "zip", "7z", "tar", "gz", "xz", "rar", "pdf", "exe", "msi", "dmg", "deb", "rpm", "appimage",
    "iso", "png", "jpg", "jpeg", "gif", "webp", "svg", "csv", "json", "txt", "epub", "docx",
    "xlsx", "pptx",
];

fn host_matches(host: &str, list: &[&str]) -> bool {
    list.iter()
        .any(|known| host == *known || host.ends_with(&format!(".{known}")))
}

/// A copied link gets the tools that use links first, with the link already filled in.
pub(crate) fn context_suggestions(text: &str) -> Vec<ContextSuggestion> {
    let trimmed = text.trim();
    let mut suggestions = Vec::new();
    let link = (!trimmed.contains(char::is_whitespace) && trimmed.len() <= 8 * 1024)
        .then(|| url::Url::parse(trimmed).ok())
        .flatten()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .and_then(|url| Some((url.host_str()?.to_ascii_lowercase(), url)));
    if let Some((host, url)) = &link {
        let host = host.strip_prefix("www.").unwrap_or(host);
        let extension = url
            .path()
            .rsplit('/')
            .next()
            .and_then(|name| name.rsplit_once('.'))
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .unwrap_or_default();
        let input = Some(trimmed);
        if host_matches(host, MEDIA_HOSTS) || MEDIA_EXTENSIONS.contains(&extension.as_str()) {
            suggestions.push(suggestion(
                "arcade.web.downloader",
                "Download this video or audio",
                input,
            ));
            if host_matches(host, TRANSCRIPT_HOSTS) {
                suggestions.push(suggestion(
                    "arcade.web.transcript",
                    "Get the transcript",
                    input,
                ));
            }
            if host_matches(host, MEDIA_HOSTS) {
                suggestions.push(suggestion(
                    "arcade.web.thumbnail",
                    "Save the thumbnail",
                    input,
                ));
            }
        } else if FILE_EXTENSIONS.contains(&extension.as_str()) {
            suggestions.push(suggestion(
                "arcade.web.file-downloader",
                "Download this file",
                input,
            ));
        } else {
            suggestions.push(suggestion(
                "arcade.web.snapshot",
                "Save this page as a PDF or picture",
                input,
            ));
            suggestions.push(suggestion(
                "arcade.web.markdown",
                "Save this page as Markdown",
                input,
            ));
        }
        suggestions.push(suggestion(
            "arcade.barcode.qr-generate",
            "Make a QR code for this link",
            None,
        ));
        return suggestions;
    }
    if serde_json::from_str::<serde_json::Value>(text).is_ok() {
        suggestions.push(suggestion(
            "arcade.text.structured",
            "Clipboard contains JSON",
            None,
        ));
    }
    if !text.is_empty() {
        suggestions.push(suggestion(
            "arcade.text.case",
            "Clipboard contains text",
            None,
        ));
    }
    suggestions
}

#[cfg(test)]
mod context_tests {
    use super::context_suggestions;

    fn ids(text: &str) -> Vec<(String, bool)> {
        context_suggestions(text)
            .into_iter()
            .map(|s| (s.tool_id, s.input.is_some()))
            .collect()
    }

    #[test]
    fn a_video_link_suggests_download_transcript_and_thumbnail_first() {
        let found = ids(" https://www.youtube.com/watch?v=dQw4w9WgXcQ\n");
        assert_eq!(
            found,
            vec![
                ("arcade.web.downloader".into(), true),
                ("arcade.web.transcript".into(), true),
                ("arcade.web.thumbnail".into(), true),
                ("arcade.barcode.qr-generate".into(), false),
            ]
        );
        let first = context_suggestions("https://youtu.be/dQw4w9WgXcQ").remove(0);
        assert_eq!(first.input.as_deref(), Some("https://youtu.be/dQw4w9WgXcQ"));
    }

    #[test]
    fn subdomains_and_media_files_count_as_media() {
        assert_eq!(
            ids("https://m.youtube.com/shorts/abc")[0].0,
            "arcade.web.downloader"
        );
        assert_eq!(
            ids("https://music.youtube.com/watch?v=x")[1].0,
            "arcade.web.transcript"
        );
        let stream = ids("https://cdn.example.com/live/index.m3u8");
        assert_eq!(stream[0].0, "arcade.web.downloader");
        assert_eq!(stream[1].0, "arcade.barcode.qr-generate");
        // Look-alike hosts are not media sites.
        assert_eq!(
            ids("https://notyoutube.com/watch")[0].0,
            "arcade.web.snapshot"
        );
    }

    #[test]
    fn file_and_page_links_get_their_own_tools() {
        assert_eq!(
            ids("https://example.com/release/app.zip")[0].0,
            "arcade.web.file-downloader"
        );
        let page = ids("https://example.com/article");
        assert_eq!(page[0], ("arcade.web.snapshot".into(), true));
        assert_eq!(page[1], ("arcade.web.markdown".into(), true));
    }

    #[test]
    fn other_text_keeps_the_text_tools() {
        assert_eq!(
            ids("{\"a\": 1}"),
            vec![
                ("arcade.text.structured".into(), false),
                ("arcade.text.case".into(), false)
            ]
        );
        assert_eq!(ids("hello there"), vec![("arcade.text.case".into(), false)]);
        assert_eq!(
            ids("ftp://example.com/file.zip"),
            vec![("arcade.text.case".into(), false)]
        );
        assert_eq!(
            ids("see https://example.com"),
            vec![("arcade.text.case".into(), false)]
        );
        assert!(ids("").is_empty());
    }
}
