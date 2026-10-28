//! Settings › Start at login. Box writes the same entry Arcade Tools turns on
//! and off for it, so the two never disagree: the receipt's
//! `~/.config/autostart/arcade-box.desktop` (through Link's installer) on
//! Linux, `~/Library/LaunchAgents/arcade.box.plist` on macOS and the Startup
//! folder's `Arcade Box.lnk` on Windows. Each starts `--background`.

use serde::Serialize;
#[cfg(not(target_os = "linux"))]
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginItem {
    /// Whether this copy can change it (an installed copy, not a dev build).
    pub available: bool,
    pub enabled: bool,
    /// Why it can't be changed, when it can't.
    pub reason: Option<String>,
}

fn unavailable(reason: &str) -> LoginItem {
    LoginItem {
        available: false,
        enabled: false,
        reason: Some(reason.into()),
    }
}

#[cfg(target_os = "linux")]
fn read() -> Result<LoginItem, String> {
    use crate::installer;
    use arcade_link::install::{self, Environment, Runtime, State};
    let environment = Environment::discover().map_err(|e| e.to_string())?;
    let runtime = Runtime::discover().map_err(|e| e.to_string())?;
    let detection =
        install::detect(&environment, installer::ME, &runtime).map_err(|e| e.to_string())?;
    if detection.state == State::DevBuild {
        return Ok(unavailable("Development builds don't start at login."));
    }
    if detection.state != State::InstalledHere {
        return Ok(unavailable("Install Arcade Box to start it at login."));
    }
    let enabled = detection
        .receipt
        .and_then(|r| r.integration.autostart)
        .is_some_and(|path| installer::login_entry_runs(&path));
    Ok(LoginItem {
        available: true,
        enabled,
        reason: None,
    })
}

#[cfg(target_os = "linux")]
fn write(enabled: bool) -> Result<(), String> {
    use crate::installer;
    use arcade_link::install::{Environment, Runtime};
    let environment = Environment::discover().map_err(|e| e.to_string())?;
    let runtime = Runtime::discover().map_err(|e| e.to_string())?;
    let mut options = installer::Options::defaults(&environment);
    options.start_at_login = enabled;
    options.remove_download = false;
    // Reinstalling the running copy over itself only rewrites its entries.
    installer::install_copy(&environment, &runtime, &options)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "linux"))]
fn entry() -> Option<PathBuf> {
    let home = arcade_link::paths::UserDirs::discover();
    if cfg!(target_os = "macos") {
        Some(home.home.join("Library/LaunchAgents/arcade.box.plist"))
    } else {
        Some(
            home.roaming
                .join("Microsoft/Windows/Start Menu/Programs/Startup/Arcade Box.lnk"),
        )
    }
}

/// The installed executable, never a copy running from a download or a
/// development build.
#[cfg(not(target_os = "linux"))]
fn installed_executable() -> Result<PathBuf, String> {
    let executable = arcade_link::manifest::current_executable();
    if arcade_link::install::is_dev_build(&executable) {
        return Err("Development builds don't start at login.".into());
    }
    #[cfg(target_os = "macos")]
    if !executable.starts_with("/Applications")
        && !executable.starts_with(
            arcade_link::paths::UserDirs::discover()
                .home
                .join("Applications"),
        )
    {
        return Err("Move Arcade Box to Applications to start it at login.".into());
    }
    Ok(executable)
}

#[cfg(not(target_os = "linux"))]
fn read() -> Result<LoginItem, String> {
    if let Err(reason) = installed_executable() {
        return Ok(unavailable(&reason));
    }
    Ok(LoginItem {
        available: true,
        enabled: entry().is_some_and(|path| path.is_file()),
        reason: None,
    })
}

#[cfg(target_os = "macos")]
fn write(enabled: bool) -> Result<(), String> {
    let path = entry().ok_or("Couldn't find the LaunchAgents folder.")?;
    if !enabled {
        return remove(&path);
    }
    let executable = installed_executable()?;
    let quoted = executable
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>arcade.box</string><key>ProgramArguments</key><array><string>{quoted}</string><string>--background</string></array><key>RunAtLoad</key><true/></dict></plist>\n"
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    arcade_link::paths::write_atomic(&path, plist.as_bytes(), false).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn write(enabled: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let path = entry().ok_or("Couldn't find the Startup folder.")?;
    if !enabled {
        return remove(&path);
    }
    let executable = installed_executable()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let quote = |p: &std::path::Path| format!("'{}'", p.to_string_lossy().replace('\'', "''"));
    let script = format!(
        "$ErrorActionPreference='Stop'; $s=(New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath={}; $s.Arguments='--background'; $s.WorkingDirectory={}; $s.Save()",
        quote(&path),
        quote(&executable),
        quote(executable.parent().unwrap_or(&executable)),
    );
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Couldn't create the login shortcut: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(not(target_os = "linux"))]
fn remove(path: &std::path::Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

#[tauri::command]
pub async fn login_item() -> Result<LoginItem, String> {
    tauri::async_runtime::spawn_blocking(read)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn set_login_item(enabled: bool) -> Result<LoginItem, String> {
    tauri::async_runtime::spawn_blocking(move || {
        write(enabled).map_err(|error| format!("Couldn't change start at login: {error}"))?;
        read()
    })
    .await
    .map_err(|e| e.to_string())?
}
