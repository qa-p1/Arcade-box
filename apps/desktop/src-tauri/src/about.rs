//! Settings › About: version and channel, who updates Box, its logs and the
//! shared "Copy diagnostics" text (STANDARD §10).

use arcade_core::Arcade;
use arcade_core::link::LinkSettings;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
    pub version: String,
    pub channel: String,
    /// Arcade Tools installs Box's updates when Tools is installed.
    pub updates_by_tools: bool,
    /// Where keys (`.env`), models and user tools go.
    pub data_folder: String,
}

fn data_folder() -> String {
    crate::link::data_dir().map_or_else(String::new, |dir| dir.to_string_lossy().into_owned())
}

fn channel() -> String {
    arcade_link::install::Environment::discover()
        .ok()
        .and_then(|e| e.receipts().read(crate::installer::ME).ok().flatten())
        .map_or_else(|| "stable".into(), |r| r.channel)
}

#[tauri::command]
pub async fn about_info() -> AboutInfo {
    tauri::async_runtime::spawn_blocking(|| AboutInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        channel: channel(),
        updates_by_tools: arcade_link::install::Environment::discover()
            .is_ok_and(|e| crate::installer::tools_installed(&e)),
        data_folder: data_folder(),
    })
    .await
    .unwrap_or_else(|_| AboutInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        channel: "stable".into(),
        updates_by_tools: false,
        data_folder: data_folder(),
    })
}

/// The shared header, then Box's own lines. No inputs, paths inside the
/// user's documents or keys.
#[tauri::command]
pub async fn diagnostics(
    runtime: tauri::State<'_, Arc<Arcade>>,
    jobs: tauri::State<'_, crate::JobManager>,
) -> Result<String, String> {
    let runtime = Arc::clone(&runtime);
    let jobs = jobs.list().len();
    tauri::async_runtime::spawn_blocking(move || {
        let mut header = arcade_link::diagnostics::Header::new(
            crate::installer::ME,
            env!("CARGO_PKG_VERSION"),
            &channel(),
        );
        header.link_enabled = LinkSettings::load(&runtime).enabled;
        header.peers_from(&arcade_link::Locations::discover());
        let mut providers = arcade_core::provider::discover_ffmpeg(None);
        providers.extend(arcade_core::provider::discover_qpdf(None));
        providers.extend(arcade_core::provider::discover_vips(None));
        providers.extend(arcade_core::provider::discover_tesseract());
        for provider in &providers {
            header.engine(&provider.capability, &provider.version);
        }
        let tools = runtime.list_tools();
        let mut text = header.render();
        text += &format!("Tools: {}\n", tools.len());
        text += &format!("Background jobs: {jobs}\n");
        text += &format!("Install: {:?}\n", crate::installer::method());
        Ok(text)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Shows Box's log folder in the file manager.
#[tauri::command]
pub fn open_logs(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let logs = crate::link::data_dir()
        .ok_or("Couldn't find Arcade Box's data folder.")?
        .join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("Couldn't open the logs: {e}"))?;
    app.opener()
        .open_path(logs.to_string_lossy(), None::<&str>)
        .map_err(|e| format!("Couldn't open the logs: {e}"))
}

/// About's "Open Tools": Tools' manifest names its executable; a running
/// Tools comes to the front instead of starting twice.
#[tauri::command]
pub fn open_tools() -> Result<(), String> {
    let registry = arcade_link::Registry::load(&arcade_link::Locations::discover());
    let manifest = registry
        .get(arcade_link::ids::TOOLS)
        .ok_or("Arcade Tools isn't installed.")?;
    let mut command = std::process::Command::new(&manifest.executable);
    arcade_link::childenv::detach_from_image(&mut command)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't open Arcade Tools: {e}"))
}
