use serde::Serialize;
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
use std::env;
#[cfg(target_os = "linux")]
use std::{env, path::PathBuf, process::Command};
use std::{thread, time::Duration};
use tauri::Manager;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPinStatus {
    pub platform: String,
    pub available: bool,
    pub message: String,
}

pub fn status() -> WindowPinStatus {
    #[cfg(target_os = "windows")]
    {
        return WindowPinStatus {
            platform: "windows".into(),
            available: true,
            message: "Pins the window that was active before Arcade Box hid. Windows may deny changes to protected or elevated windows.".into(),
        };
    }
    #[cfg(target_os = "linux")]
    {
        let wayland = env::var_os("WAYLAND_DISPLAY").is_some_and(|display| !display.is_empty())
            || env::var("XDG_SESSION_TYPE")
                .is_ok_and(|session| session.eq_ignore_ascii_case("wayland"));
        let wmctrl = find_wmctrl();
        return WindowPinStatus {
            platform: if wayland { "wayland" } else { "x11" }.into(),
            available: !wayland && wmctrl.is_some(),
            message: if wayland {
                "Wayland does not expose a general always-on-top API to desktop apps. Use the window manager's own controls.".into()
            } else if wmctrl.is_some() {
                "Pins the window that was active before Arcade Box hid; the X11 window manager must support EWMH above state.".into()
            } else {
                "X11 window pinning requires wmctrl, which was not found on PATH.".into()
            },
        };
    }
    #[cfg(target_os = "macos")]
    {
        WindowPinStatus {
            platform: "macos".into(),
            available: false,
            message: "macOS does not provide a general API to set another app's window above all windows. Use the app's own window controls.".into(),
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        WindowPinStatus {
            platform: env::consts::OS.into(),
            available: false,
            message: "Window pinning is not supported on this platform.".into(),
        }
    }
}

pub fn set_foreground_pin(pin: bool, app: &tauri::AppHandle) -> Result<WindowPinStatus, String> {
    let status = status();
    if !status.available {
        return Err(status.message);
    }
    let window = app.get_webview_window("island");
    if let Some(window) = &window {
        window.hide().map_err(|error| error.to_string())?;
    }
    thread::sleep(Duration::from_millis(120));
    let result = set_previous_foreground_pin(pin);
    match result {
        Ok(()) => Ok(WindowPinStatus {
            message: if pin {
                "The previously active window was set above other windows.".into()
            } else {
                "The previously active window was removed from the always-on-top state.".into()
            },
            ..status
        }),
        Err(error) => {
            if let Some(window) = window {
                let _ = window.show();
            }
            Err(error)
        }
    }
}

#[cfg(target_os = "windows")]
fn set_previous_foreground_pin(pin: bool) -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId, HWND_NOTOPMOST, HWND_TOPMOST,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
        },
    };

    let foreground: HWND = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return Err("No active window was available to pin".into());
    }
    let mut process_id = 0;
    unsafe { GetWindowThreadProcessId(foreground, &mut process_id) };
    if process_id == std::process::id() {
        return Err("Arcade Box cannot pin its own tool window from this action".into());
    }
    let insert_after = if pin { HWND_TOPMOST } else { HWND_NOTOPMOST };
    let succeeded = unsafe {
        SetWindowPos(
            foreground,
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };
    if succeeded == 0 {
        Err("Windows denied the window pin request".into())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn set_previous_foreground_pin(pin: bool) -> Result<(), String> {
    if env::var_os("WAYLAND_DISPLAY").is_some_and(|display| !display.is_empty())
        || env::var("XDG_SESSION_TYPE").is_ok_and(|session| session.eq_ignore_ascii_case("wayland"))
    {
        return Err("Wayland does not allow a general window pin request".into());
    }
    let wmctrl = find_wmctrl().ok_or("wmctrl is not available on PATH")?;
    let state = if pin { "add,above" } else { "remove,above" };
    let output = Command::new(wmctrl)
        .args(["-r", ":ACTIVE:", "-b", state])
        .output()
        .map_err(|error| format!("Could not run wmctrl: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if detail.is_empty() {
            "The X11 window manager rejected the pin request".into()
        } else {
            detail
        })
    }
}

#[cfg(target_os = "macos")]
fn set_previous_foreground_pin(_: bool) -> Result<(), String> {
    Err("macOS does not expose a general window pin API for other applications".into())
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn set_previous_foreground_pin(_: bool) -> Result<(), String> {
    Err("Window pinning is not supported on this platform".into())
}

#[cfg(target_os = "linux")]
fn find_wmctrl() -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|directory| directory.join("wmctrl"))
        .find(|candidate| {
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
        .and_then(|path| path.canonicalize().ok())
}
