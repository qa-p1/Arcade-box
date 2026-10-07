//! Explicitly opted-in local clipboard history. Clipboard contents are stored
//! separately from tool/job history and are never copied into diagnostic logs.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::State;
use tauri_plugin_clipboard_manager::ClipboardExt;

const MAX_ITEMS: usize = 500;
const MAX_PINNED: usize = 100;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_IMAGE_EDGE: u32 = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardHistoryItem {
    pub id: String,
    pub kind: String,
    pub text: Option<String>,
    pub image_rgba_base64: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub preview: String,
    pub captured_at: u64,
    pub pinned: bool,
    pub source_application: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredHistory {
    enabled: bool,
    retention_days: u16,
    excluded_applications: Vec<String>,
    items: Vec<ClipboardHistoryItem>,
}

impl Default for StoredHistory {
    fn default() -> Self {
        Self {
            enabled: false,
            retention_days: 7,
            excluded_applications: Vec::new(),
            items: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardHistoryStatus {
    pub enabled: bool,
    pub retention_days: u16,
    pub item_count: usize,
    pub pinned_count: usize,
    pub source_application_available: bool,
    pub excluded_applications: Vec<String>,
    pub message: &'static str,
}

#[derive(Clone)]
pub struct ClipboardHistory {
    path: PathBuf,
    inner: Arc<Mutex<StoredHistory>>,
    monitor_started: Arc<AtomicBool>,
}

impl ClipboardHistory {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let history = if path.exists() {
            let bytes = fs::read(&path)
                .map_err(|error| format!("Could not read clipboard history: {error}"))?;
            serde_json::from_slice::<StoredHistory>(&bytes)
                .map_err(|error| format!("Could not decode clipboard history: {error}"))?
        } else {
            StoredHistory::default()
        };
        let service = Self {
            path,
            inner: Arc::new(Mutex::new(history)),
            monitor_started: Arc::new(AtomicBool::new(false)),
        };
        service.expire_and_save()?;
        Ok(service)
    }

    pub fn start_monitor(&self, app: tauri::AppHandle) {
        if self.monitor_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let service = self.clone();
        let _ = std::thread::Builder::new()
            .name("arcade-clipboard-history".into())
            .spawn(move || {
                // Fingerprint of the last clipboard content seen. An unchanged
                // clipboard skips the foreground lookup, copy, and store work.
                let mut last_seen: Option<u64> = None;
                loop {
                    std::thread::sleep(Duration::from_millis(1200));
                    if !service.is_enabled() {
                        last_seen = None;
                        continue;
                    }
                    if let Ok(text) = app.clipboard().read_text()
                        && !text.trim().is_empty()
                    {
                        let mut hasher = DefaultHasher::new();
                        "text".hash(&mut hasher);
                        text.hash(&mut hasher);
                        let fingerprint = hasher.finish();
                        if last_seen != Some(fingerprint) {
                            let source = foreground_application_name();
                            if service.capture_text(text, source).unwrap_or(false) {
                                last_seen = Some(fingerprint);
                            }
                        }
                        continue;
                    }
                    if let Ok(image) = app.clipboard().read_image() {
                        let width = image.width();
                        let height = image.height();
                        if !valid_image_dimensions(width, height) {
                            let mut hasher = DefaultHasher::new();
                            "oversized-image".hash(&mut hasher);
                            width.hash(&mut hasher);
                            height.hash(&mut hasher);
                            last_seen = Some(hasher.finish());
                            continue;
                        }
                        let mut hasher = DefaultHasher::new();
                        "image".hash(&mut hasher);
                        width.hash(&mut hasher);
                        height.hash(&mut hasher);
                        image.rgba().hash(&mut hasher);
                        let fingerprint = hasher.finish();
                        if last_seen == Some(fingerprint) {
                            continue;
                        }
                        let rgba = image.rgba().to_vec();
                        let source = foreground_application_name();
                        drop(image);
                        if service
                            .capture_image(rgba, width, height, source)
                            .unwrap_or(false)
                        {
                            last_seen = Some(fingerprint);
                        }
                    }
                }
            });
    }

    fn is_enabled(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .enabled
    }

    pub fn status(&self) -> ClipboardHistoryStatus {
        let state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ClipboardHistoryStatus {
            enabled: state.enabled,
            retention_days: state.retention_days,
            item_count: state.items.len(),
            pinned_count: state.items.iter().filter(|item| item.pinned).count(),
            source_application_available: source_application_supported(),
            excluded_applications: state.excluded_applications.clone(),
            message: if state.enabled {
                "Clipboard history is enabled. Captured content stays in its private local store."
            } else {
                "Clipboard history is off. Arcade Box will not read clipboard contents for history."
            },
        }
    }

    pub fn list(&self, query: &str) -> Vec<ClipboardHistoryItem> {
        let state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let query = query.trim().to_lowercase();
        state
            .items
            .iter()
            .filter(|item| {
                query.is_empty()
                    || item.preview.to_lowercase().contains(&query)
                    || item
                        .text
                        .as_ref()
                        .is_some_and(|text| text.to_lowercase().contains(&query))
                    || item
                        .source_application
                        .as_ref()
                        .is_some_and(|name| name.to_lowercase().contains(&query))
            })
            .cloned()
            .collect()
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<ClipboardHistoryStatus, String> {
        {
            let mut state = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.enabled = enabled;
            self.save_locked(&state)?;
        }
        Ok(self.status())
    }

    pub fn set_retention_days(
        &self,
        retention_days: u16,
    ) -> Result<ClipboardHistoryStatus, String> {
        if !matches!(retention_days, 1 | 7 | 30 | 90) {
            return Err("Choose a retention period of 1, 7, 30, or 90 days".into());
        }
        {
            let mut state = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.retention_days = retention_days;
            expire_items(&mut state);
            self.save_locked(&state)?;
        }
        Ok(self.status())
    }

    pub fn set_excluded_applications(
        &self,
        names: Vec<String>,
    ) -> Result<ClipboardHistoryStatus, String> {
        if names.len() > 64
            || names.iter().any(|name| {
                name.trim().is_empty() || name.len() > 160 || name.chars().any(char::is_control)
            })
        {
            return Err(
                "Application exclusions must be up to 64 non-empty names of 160 characters or less"
                    .into(),
            );
        }
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.excluded_applications = names
            .into_iter()
            .map(|name| name.trim().to_lowercase())
            .filter(|name| !name.is_empty())
            .collect();
        self.save_locked(&state)?;
        drop(state);
        Ok(self.status())
    }

    pub fn pin(&self, id: &str, pinned: bool) -> Result<(), String> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if pinned && state.items.iter().filter(|item| item.pinned).count() >= MAX_PINNED {
            return Err(format!(
                "Clipboard history can keep up to {MAX_PINNED} items pinned"
            ));
        }
        let item = state
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or("Clipboard history item was not found")?;
        item.pinned = pinned;
        self.save_locked(&state)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = state.items.len();
        state.items.retain(|item| item.id != id);
        if previous == state.items.len() {
            return Err("Clipboard history item was not found".into());
        }
        self.save_locked(&state)
    }

    pub fn clear(&self) -> Result<(), String> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.items.clear();
        self.save_locked(&state)
    }

    pub fn copy_item(&self, id: &str, app: &tauri::AppHandle) -> Result<(), String> {
        let item = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .items
            .iter()
            .find(|item| item.id == id)
            .cloned()
            .ok_or("Clipboard history item was not found")?;
        if let Some(text) = item.text {
            return app
                .clipboard()
                .write_text(text)
                .map_err(|error| error.to_string());
        }
        if item.kind == "image" {
            let width = item.image_width.ok_or("Clipboard image width is missing")?;
            let height = item
                .image_height
                .ok_or("Clipboard image height is missing")?;
            let encoded = item
                .image_rgba_base64
                .as_deref()
                .ok_or("Clipboard image pixels are missing")?;
            let rgba = STANDARD
                .decode(encoded)
                .map_err(|_| "Clipboard image data is invalid")?;
            let expected = (width as usize)
                .saturating_mul(height as usize)
                .saturating_mul(4);
            if width == 0
                || height == 0
                || width > MAX_IMAGE_EDGE
                || height > MAX_IMAGE_EDGE
                || rgba.len() != expected
                || rgba.len() > MAX_IMAGE_BYTES
            {
                return Err("Clipboard image data is outside the supported size limits".into());
            }
            let image = tauri::image::Image::new_owned(rgba, width, height);
            return app
                .clipboard()
                .write_image(&image)
                .map_err(|error| error.to_string());
        }
        Err("This clipboard item cannot be restored as text".into())
    }

    fn capture_text(
        &self,
        text: String,
        source_application: Option<String>,
    ) -> Result<bool, String> {
        if text.len() > MAX_TEXT_BYTES || is_sensitive_text(&text) {
            return Ok(true);
        }
        let mut hasher = DefaultHasher::new();
        "text".hash(&mut hasher);
        text.hash(&mut hasher);
        let id = format!("clip-{:016x}", hasher.finish());
        let source_application = source_application.map(|name| name.to_lowercase());
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.enabled || source_is_excluded(&state, source_application.as_deref()) {
            return Ok(false);
        }
        if state.items.first().is_some_and(|item| item.id == id) {
            return Ok(true);
        }
        let preview = text
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(160)
            .collect::<String>();
        let captured_at = unix_now();
        state.items.insert(
            0,
            ClipboardHistoryItem {
                id,
                kind: if is_file_reference(&text) {
                    "file"
                } else {
                    "text"
                }
                .into(),
                text: Some(text),
                image_rgba_base64: None,
                image_width: None,
                image_height: None,
                preview,
                captured_at,
                pinned: false,
                source_application,
            },
        );
        trim_history(&mut state);
        self.save_locked(&state)?;
        Ok(true)
    }

    fn capture_image(
        &self,
        rgba: Vec<u8>,
        width: u32,
        height: u32,
        source_application: Option<String>,
    ) -> Result<bool, String> {
        let expected = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        if !valid_image_dimensions(width, height)
            || rgba.len() != expected
            || rgba.len() > MAX_IMAGE_BYTES
        {
            return Ok(true);
        }
        let mut hasher = DefaultHasher::new();
        "image".hash(&mut hasher);
        width.hash(&mut hasher);
        height.hash(&mut hasher);
        rgba.hash(&mut hasher);
        let id = format!("clip-{:016x}", hasher.finish());
        let source_application = source_application.map(|name| name.to_lowercase());
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.enabled || source_is_excluded(&state, source_application.as_deref()) {
            return Ok(false);
        }
        if state.items.first().is_some_and(|item| item.id == id) {
            return Ok(true);
        }
        state.items.insert(
            0,
            ClipboardHistoryItem {
                id,
                kind: "image".into(),
                text: None,
                image_rgba_base64: Some(STANDARD.encode(rgba)),
                image_width: Some(width),
                image_height: Some(height),
                preview: format!("Image · {width} × {height}"),
                captured_at: unix_now(),
                pinned: false,
                source_application,
            },
        );
        trim_history(&mut state);
        self.save_locked(&state)?;
        Ok(true)
    }

    fn expire_and_save(&self) -> Result<(), String> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        expire_items(&mut state);
        self.save_locked(&state)
    }

    fn save_locked(&self, state: &StoredHistory) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Could not create private clipboard storage: {error}"))?;
            set_private_directory_permissions(parent)?;
        }
        let bytes = serde_json::to_vec(state).map_err(|error| error.to_string())?;
        let mut temporary = self.path.clone();
        temporary.set_extension("json.tmp");
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("Could not save clipboard history: {error}"))?;
        file.write_all(&bytes)
            .map_err(|error| format!("Could not save clipboard history: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Could not finish saving clipboard history: {error}"))?;
        drop(file);
        set_private_file_permissions(&temporary)?;
        fs::rename(&temporary, &self.path)
            .map_err(|error| format!("Could not publish clipboard history: {error}"))
    }
}

fn source_is_excluded(state: &StoredHistory, source: Option<&str>) -> bool {
    source.is_some_and(|source| {
        let source = normalized_application_name(source);
        source.contains("arcade")
            || state.excluded_applications.iter().any(|excluded| {
                let excluded = normalized_application_name(excluded);
                !excluded.is_empty() && source.contains(&excluded)
            })
    })
}

fn normalized_application_name(name: &str) -> String {
    name.chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn is_sensitive_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "-----begin private key-----",
        "-----begin rsa private key-----",
        "-----begin ec private key-----",
        "-----begin dsa private key-----",
        "-----begin openpgp private key-----",
        "-----begin openssh private key-----",
        "-----begin encrypted private key-----",
        "authorization: bearer ",
        "authorization: basic ",
        "bearer eyj",
        "api_key=",
        "api-key=",
        "api key:",
        "access_token=",
        "refresh_token=",
        "password=",
        "password =",
        "password:",
        "password :",
        "passwd=",
        "passwd:",
        "pwd=",
        "passphrase=",
        "passphrase:",
        "client_secret=",
        "client-secret=",
        "secret_key=",
        "secret-key=",
        "api_token=",
        "private_key=",
        "aws_secret_access_key=",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn is_file_reference(text: &str) -> bool {
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    !lines.is_empty() && lines.iter().all(|line| line.starts_with("file://"))
}

fn trim_history(state: &mut StoredHistory) {
    expire_items(state);
    while state.items.len() > MAX_ITEMS {
        let Some(index) = state.items.iter().rposition(|item| !item.pinned) else {
            break;
        };
        state.items.remove(index);
    }
}

fn expire_items(state: &mut StoredHistory) {
    let expiry = unix_now().saturating_sub(u64::from(state.retention_days) * 24 * 60 * 60);
    state
        .items
        .retain(|item| item.pinned || item.captured_at >= expiry);
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(unix)]
fn set_private_directory_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Could not secure clipboard storage directory: {error}"))
}

#[cfg(not(unix))]
fn set_private_directory_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("Could not secure clipboard history file: {error}"))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn foreground_application_name() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        // Windows exposes the process owning the foreground window without an
        // accessibility grant. Keep the returned value to a basename only.
        return windows_foreground_application_name();
    }
    #[cfg(target_os = "macos")]
    {
        // Accessibility permissions may be required. A failed lookup leaves
        // capture active but does not apply application-name exclusions.
        return macos_frontmost_application_name();
    }
    #[cfg(target_os = "linux")]
    {
        // Wayland deliberately does not expose another application's active
        // window to ordinary clients. X11 helpers are optional and queried
        // without a shell.
        return linux_active_application_name();
    }
    #[allow(unreachable_code)]
    None
}

fn source_application_supported() -> bool {
    #[cfg(target_os = "windows")]
    {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        return !is_wayland_session()
            && std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty())
            && executable_on_path("xdotool");
    }
    #[allow(unreachable_code)]
    false
}

#[cfg(target_os = "windows")]
fn windows_foreground_application_name() -> Option<String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    };
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return None;
    }
    let mut process_id = 0;
    unsafe { GetWindowThreadProcessId(window, &mut process_id) };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return None;
    }
    let mut buffer = vec![0u16; 32_768];
    let mut length = buffer.len() as u32;
    let success = unsafe {
        QueryFullProcessImageNameW(process as HANDLE, 0, buffer.as_mut_ptr(), &mut length)
    };
    unsafe { CloseHandle(process as HANDLE) };
    if success == 0 {
        return None;
    }
    let path = std::ffi::OsString::from_wide(&buffer[..length as usize]);
    Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

#[cfg(target_os = "macos")]
fn macos_frontmost_application_name() -> Option<String> {
    let output = std::process::Command::new("osascript")
        .args(["-e", "tell application \"System Events\" to get name of first application process whose frontmost is true"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(target_os = "linux")]
fn linux_active_application_name() -> Option<String> {
    if is_wayland_session() {
        return None;
    }
    let output = std::process::Command::new("xdotool")
        .args(["getwindowfocus", "getwindowpid"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let pid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let comm_path = PathBuf::from("/proc").join(pid).join("comm");
    fs::read_to_string(comm_path)
        .ok()
        .map(|name| name.trim().to_owned())
}

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|display| !display.is_empty())
        || std::env::var("XDG_SESSION_TYPE")
            .is_ok_and(|session| session.eq_ignore_ascii_case("wayland"))
}

#[cfg(target_os = "linux")]
fn executable_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(name);
        let Ok(metadata) = candidate.metadata() else {
            return false;
        };
        if !metadata.is_file() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            true
        }
    })
}

fn valid_image_dimensions(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= MAX_IMAGE_EDGE
        && height <= MAX_IMAGE_EDGE
        && (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
            <= MAX_IMAGE_BYTES
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn windows_foreground_application_name() -> Option<String> {
    None
}

#[tauri::command]
pub fn clipboard_history_status(history: State<'_, ClipboardHistory>) -> ClipboardHistoryStatus {
    history.status()
}

#[tauri::command]
pub fn list_clipboard_history(
    query: String,
    history: State<'_, ClipboardHistory>,
) -> Vec<ClipboardHistoryItem> {
    history.list(&query)
}

#[tauri::command]
pub fn set_clipboard_history_enabled(
    enabled: bool,
    history: State<'_, ClipboardHistory>,
) -> Result<ClipboardHistoryStatus, String> {
    history.set_enabled(enabled)
}

#[tauri::command]
pub fn set_clipboard_history_retention(
    days: u16,
    history: State<'_, ClipboardHistory>,
) -> Result<ClipboardHistoryStatus, String> {
    history.set_retention_days(days)
}

#[tauri::command]
pub fn set_clipboard_history_exclusions(
    applications: Vec<String>,
    history: State<'_, ClipboardHistory>,
) -> Result<ClipboardHistoryStatus, String> {
    history.set_excluded_applications(applications)
}

#[tauri::command]
pub fn pin_clipboard_history_item(
    id: String,
    pinned: bool,
    history: State<'_, ClipboardHistory>,
) -> Result<(), String> {
    history.pin(&id, pinned)
}

#[tauri::command]
pub fn delete_clipboard_history_item(
    id: String,
    history: State<'_, ClipboardHistory>,
) -> Result<(), String> {
    history.delete(&id)
}

#[tauri::command]
pub fn clear_clipboard_history(history: State<'_, ClipboardHistory>) -> Result<(), String> {
    history.clear()
}

#[tauri::command]
pub fn copy_clipboard_history_item(
    id: String,
    app: tauri::AppHandle,
    history: State<'_, ClipboardHistory>,
) -> Result<(), String> {
    history.copy_item(&id, &app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_history_requires_current_explicit_opt_in_and_respects_exclusions() {
        let directory = tempfile::tempdir().unwrap();
        let history = ClipboardHistory::open(directory.path().join("history.json")).unwrap();

        assert!(
            !history
                .capture_text("captured while disabled".into(), None)
                .unwrap()
        );
        assert!(history.list("").is_empty());

        history.set_enabled(true).unwrap();
        history
            .set_excluded_applications(vec!["Password Manager".into()])
            .unwrap();
        assert!(
            !history
                .capture_text("excluded secret".into(), Some("password-manager".into()))
                .unwrap()
        );
        assert!(
            history
                .capture_text("kept text".into(), Some("text-editor".into()))
                .unwrap()
        );
        history.set_enabled(false).unwrap();
        assert!(
            !history
                .capture_text("captured during disable race".into(), None)
                .unwrap()
        );

        let items = history.list("");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text.as_deref(), Some("kept text"));
    }

    #[test]
    fn clipboard_history_skips_sensitive_text_and_enforces_content_caps() {
        assert!(is_sensitive_text("Password: hunter2"));
        assert!(is_sensitive_text("Authorization: Bearer abc123"));
        assert!(is_sensitive_text("-----BEGIN EC PRIVATE KEY-----"));
        assert!(is_sensitive_text("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert!(!is_sensitive_text("I forgot my password yesterday"));
        assert!(is_file_reference(
            "file:///home/user/a.txt\nfile:///home/user/b.txt"
        ));
        assert!(!is_file_reference("\n  \n"));
        assert!(valid_image_dimensions(1024, 1024));
        assert!(!valid_image_dimensions(2048, 2048));
        assert!(!valid_image_dimensions(0, 1));

        let directory = tempfile::tempdir().unwrap();
        let history = ClipboardHistory::open(directory.path().join("history.json")).unwrap();
        history.set_enabled(true).unwrap();
        assert!(
            history
                .capture_text("x".repeat(MAX_TEXT_BYTES + 1), None)
                .unwrap()
        );
        assert!(history.capture_image(vec![0; 4], 2, 2, None).unwrap());
        assert!(history.list("").is_empty());
    }
}
