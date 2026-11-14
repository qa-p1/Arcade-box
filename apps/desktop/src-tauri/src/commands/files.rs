//! Files and folders the user grants, output folders, and saving, revealing or copying results.

use crate::*;

#[tauri::command]
pub(crate) fn select_files(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Vec<SelectedFile>, String> {
    let Some(paths) = app.dialog().file().blocking_pick_files() else {
        return Ok(vec![]);
    };
    paths
        .into_iter()
        .map(|file| {
            let path = file.into_path().map_err(|error| error.to_string())?;
            runtime
                .grants()
                .grant(&path)
                .map_err(|error| error.to_string())
        })
        .collect()
}

#[tauri::command]
pub(crate) fn choose_output_directory(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Option<SelectedDirectory>, String> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let path = folder.into_path().map_err(|error| error.to_string())?;
    runtime
        .grant_output_directory(&path)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn select_input_folder(
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Option<SelectedDirectory>, String> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let path = folder.into_path().map_err(|error| error.to_string())?;
    runtime
        .grant_input_directory(&path)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn revoke_input_folder(token: String, runtime: tauri::State<'_, Arc<Arcade>>) {
    runtime.revoke_input_directory(&token);
}

#[tauri::command]
pub(crate) fn revoke_output_directory(token: String, runtime: tauri::State<'_, Arc<Arcade>>) {
    runtime.revoke_output_directory(&token);
}

#[tauri::command(async)]
pub(crate) async fn save_artifact_as(
    token: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Option<SelectedFile>, String> {
    let source_path = runtime
        .grants()
        .resolve(&token)
        .map_err(|error| error.to_string())?;
    let default_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Arcade-result");
    let mut dialog = app.dialog().file().set_file_name(default_name);
    if let Some(folder) = save_folder_setting(&runtime) {
        dialog = dialog.set_directory(folder);
    }
    let Some(destination) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    let destination = destination.into_path().map_err(|error| error.to_string())?;
    runtime
        .save_artifact_as(&token, &destination)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// Settings › General › Save folder: where "Save…" starts, when it still
/// exists. Unset, the dialog starts where the desktop chooses.
pub(crate) const SAVE_FOLDER: &str = "save_folder";

pub(crate) fn save_folder_setting(runtime: &Arcade) -> Option<std::path::PathBuf> {
    runtime
        .storage()
        .setting(SAVE_FOLDER)
        .ok()
        .flatten()
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_dir())
}

#[tauri::command]
pub(crate) fn save_folder(runtime: tauri::State<'_, Arc<Arcade>>) -> Option<String> {
    save_folder_setting(&runtime).map(|path| path.to_string_lossy().into_owned())
}

/// Picks the save folder, or clears it with `reset`.
#[tauri::command(async)]
pub(crate) async fn choose_save_folder(
    reset: bool,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Option<String>, String> {
    if reset {
        runtime
            .storage()
            .set_setting(SAVE_FOLDER, "")
            .map_err(|error| format!("Couldn't reset the save folder: {error}"))?;
        return Ok(None);
    }
    let mut dialog = app.dialog().file();
    if let Some(folder) = save_folder_setting(&runtime) {
        dialog = dialog.set_directory(folder);
    }
    let Some(folder) = dialog.blocking_pick_folder() else {
        return Ok(save_folder_setting(&runtime).map(|path| path.to_string_lossy().into_owned()));
    };
    let folder = folder.into_path().map_err(|error| error.to_string())?;
    runtime
        .storage()
        .set_setting(SAVE_FOLDER, &folder.to_string_lossy())
        .map_err(|error| format!("Couldn't save the folder: {error}"))?;
    Ok(Some(folder.to_string_lossy().into_owned()))
}

#[tauri::command]
pub(crate) fn reveal_artifact(
    token: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    let path = runtime
        .grants()
        .resolve(&token)
        .map_err(|error| error.to_string())?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn open_artifact(
    token: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    let path = runtime
        .grants()
        .resolve(&token)
        .map_err(|error| error.to_string())?;
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn copy_text(text: String, app: tauri::AppHandle) -> Result<(), String> {
    if text.len() > 16 * 1024 * 1024 {
        return Err("The result is too large to copy. Save it as a file instead.".into());
    }
    app.clipboard()
        .write_text(text)
        .map_err(|error| error.to_string())
}
