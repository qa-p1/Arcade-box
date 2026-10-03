//! Platform keyboard shortcuts for application-provided plain-text paste.
//! Clipboard contents are left untouched.

use serde::Serialize;
use tauri_plugin_clipboard_manager::ClipboardExt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PastePlainStatus {
    pub platform: &'static str,
    pub available: bool,
    pub shortcut: &'static str,
    pub message: String,
}

pub fn capability_status() -> PastePlainStatus {
    #[cfg(target_os = "windows")]
    {
        return PastePlainStatus {
            platform: "windows",
            available: true,
            shortcut: "Ctrl+Shift+V",
            message: "Arcade Box will request the common plain-text paste shortcut. The clipboard remains unchanged; the target app must support this shortcut.",
        };
    }
    #[cfg(target_os = "macos")]
    {
        return PastePlainStatus {
            platform: "macos",
            available: true,
            shortcut: "⌘+⌥+⇧+V",
            message: "Arcade Box will request Paste and Match Style. macOS may ask for Accessibility permission, and the target app must support this shortcut.",
        };
    }
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let xdotool = executable_on_path("xdotool");
        return PastePlainStatus {
            platform: if wayland { "wayland" } else { "x11" },
            available: !wayland && xdotool,
            shortcut: "Ctrl+Shift+V",
            message: if wayland {
                "Wayland does not permit applications to synthesize global paste input. Copy the text, switch to the target app, and use its plain-text paste shortcut."
            } else if xdotool {
                "Arcade Box will request the common plain-text paste shortcut in the previously focused app."
            } else {
                "X11 plain-text paste needs xdotool on PATH. The clipboard will remain unchanged."
            }
            .into(),
        };
    }
    #[allow(unreachable_code)]
    PastePlainStatus {
        platform: "unsupported",
        available: false,
        shortcut: "",
        message: "Plain-text paste is not available on this platform build.".into(),
    }
}

fn executable_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| directory.join(name).is_file())
}

#[tauri::command]
pub fn paste_plain_status() -> PastePlainStatus {
    capability_status()
}

#[tauri::command(async)]
pub async fn paste_plain_text(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    let capability = capability_status();
    if !capability.available {
        return Err(capability.message);
    }
    let text = app
        .clipboard()
        .read_text()
        .map_err(|error| format!("The clipboard does not contain plain text: {error}"))?;
    if text.len() > 16 * 1024 * 1024 {
        return Err("Clipboard text exceeds the interactive limit".into());
    }
    if text.is_empty() {
        return Err("The clipboard is empty".into());
    }

    window.hide().map_err(|error| error.to_string())?;
    tokio::time::sleep(std::time::Duration::from_millis(140)).await;
    let paste_result = tauri::async_runtime::spawn_blocking(send_plain_paste_shortcut)
        .await
        .map_err(|error| format!("Could not send paste shortcut: {error}"))?;
    if let Err(error) = paste_result {
        let _ = window.show();
        let _ = window.set_focus();
        return Err(error);
    }
    Ok(())
}

fn send_plain_paste_shortcut() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VK_CONTROL,
            VK_SHIFT, VK_V,
        };
        let key = |virtual_key: u16, flags: u32| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: virtual_key,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [
            key(VK_CONTROL, 0),
            key(VK_SHIFT, 0),
            key(VK_V, 0),
            key(VK_V, KEYEVENTF_KEYUP),
            key(VK_SHIFT, KEYEVENTF_KEYUP),
            key(VK_CONTROL, KEYEVENTF_KEYUP),
        ];
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                size_of::<INPUT>() as i32,
            )
        };
        if sent != inputs.len() as u32 {
            return Err("Windows did not accept the plain-text paste shortcut".into());
        }
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to keystroke \"v\" using {command down, option down, shift down}"])
            .status()
            .map_err(|error| format!("Could not request macOS Paste and Match Style: {error}"))?;
        return status
            .success()
            .then_some(())
            .ok_or_else(|| "macOS could not send the paste shortcut. Check Arcade Box Accessibility permission.".into());
    }
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            return Err(capability_status().message);
        }
        let status = std::process::Command::new("xdotool")
            .args(["key", "--clearmodifiers", "ctrl+shift+v"])
            .status()
            .map_err(|error| format!("Could not request the X11 paste shortcut: {error}"))?;
        return status
            .success()
            .then_some(())
            .ok_or_else(|| "X11 did not accept the plain-text paste shortcut".into());
    }
    #[allow(unreachable_code)]
    Err("Plain-text paste is not available on this platform build".into())
}
