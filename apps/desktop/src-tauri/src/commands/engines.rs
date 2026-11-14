//! The external engines Box finds, and the Tesseract language download.

use crate::*;

#[tauri::command(async)]
pub(crate) fn list_providers(
    recheck: Option<bool>,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Vec<ProviderInfo> {
    let mut providers = discover_ffmpeg(None);
    providers.extend(discover_qpdf(None));
    providers.extend(discover_vips(None));
    providers.extend(arcade_core::provider::discover_tesseract());
    // BR-8: every system engine an implemented tool calls, so a missing one shows before a tool fails.
    providers.extend(arcade_core::provider::discover_poppler());
    providers.extend(arcade_core::provider::discover_ytdlp());
    providers.extend(arcade_core::provider::discover_curl());
    providers.extend(arcade_core::provider::discover_arcade(&runtime));
    // Only when the user opens Engines: re-checking every provider runs each
    // tool's `--version`, which startup must not pay for.
    if recheck.unwrap_or(false) {
        link::providers_checked(&app);
    }
    providers
}

/// Size in MB of the Tesseract download offered on this platform, if any.
#[tauri::command]
pub(crate) fn tesseract_download_size() -> Option<u32> {
    arcade_link::engines::tesseract_source().map(|source| source.size_mb)
}

/// Gets Tesseract for every Arcade app (nothing is shipped with Box).
#[tauri::command(async)]
pub(crate) async fn download_tesseract() -> Result<String, String> {
    use arcade_link::engines::{Download, download_tesseract};
    match tauri::async_runtime::spawn_blocking(download_tesseract)
        .await
        .map_err(|error| error.to_string())??
    {
        Download::Ready(_) => Ok("Tesseract is ready.".into()),
        Download::InstallerOpened => {
            Ok("Finish the Tesseract installer, then open Engines again.".into())
        }
    }
}
