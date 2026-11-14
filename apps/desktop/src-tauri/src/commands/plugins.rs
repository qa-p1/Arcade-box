//! Plugin packages: choose, preview, install and uninstall.

use crate::*;

#[tauri::command]
pub(crate) fn choose_plugin_package(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let path = folder.into_path().map_err(|error| error.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub(crate) fn preview_plugin(
    source_dir: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<arcade_core::PluginPreview, String> {
    let path = PathBuf::from(source_dir);
    if !path.is_absolute() {
        return Err("Choose an unpacked plugin package from disk".into());
    }
    runtime
        .preview_plugin(&path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn list_plugins(
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<Vec<arcade_core::PluginSummary>, String> {
    runtime.list_plugins().map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn install_plugin(
    source_dir: String,
    grants: Vec<String>,
    acknowledge_escalation: bool,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<arcade_core::PluginSummary, String> {
    let path = PathBuf::from(source_dir);
    if !path.is_absolute() {
        return Err("Choose an unpacked plugin package from disk".into());
    }
    let grants = grants
        .into_iter()
        .map(|grant| match grant.as_str() {
            "read-user-selected" => Ok(PluginPermissionGrant::ReadUserSelectedFiles),
            _ => Err(format!("Unsupported plugin permission: {grant}")),
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    runtime
        .install_plugin(
            &path,
            PluginInstallApproval {
                grants,
                acknowledge_escalation,
            },
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn uninstall_plugin(
    plugin_id: String,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<(), String> {
    runtime
        .uninstall_plugin(&plugin_id)
        .map_err(|error| error.to_string())
}
