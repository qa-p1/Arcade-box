//! Optional, process-scoped Hyprland support for the Island surface.
//!
//! Hyprland 0.55+ uses Lua as its configuration provider. These calls use
//! `hyprctl repl`/`eval` to add only in-memory bindings and to target the
//! current process's exact window address. No Hyprland config file is read or
//! written.

use serde_json::Value;
use std::env;

pub fn is_hyprland() -> bool {
    env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|signature| !signature.is_empty())
        && env::var_os("WAYLAND_DISPLAY").is_some_and(|display| !display.is_empty())
}

/// Install a temporary Hyprland key binding that activates the portal shortcut.
pub fn install_global_binding(
    app_id: &str,
    shortcut_id: &str,
    trigger: &str,
) -> Result<(), String> {
    if !is_hyprland() {
        return Ok(());
    }
    install_surface_rules()?;
    let target = format!("{app_id}:{shortcut_id}");
    if !target
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        return Err("The registered portal shortcut has an invalid identifier".into());
    }

    let published = hyprctl(&["globalshortcuts"])?;
    let published = String::from_utf8_lossy(&published);
    if !published
        .lines()
        .any(|line| line.starts_with(&format!("{target} ->")))
    {
        return Err("Hyprland did not publish the registered Arcade Box portal shortcut".into());
    }

    let (chord, mask, key) = shortcut_chord(trigger)?;
    reject_shortcut_conflict(&key, mask)?;
    let lua = format!(
        "(function() if _G.arcadeBoxGlobalShortcut then _G.arcadeBoxGlobalShortcut:remove() end; _G.arcadeBoxGlobalShortcut = hl.bind(\"{chord}\", hl.dsp.global(\"{target}\"), {{description=\"Arcade Box\"}}); return \"ok\" end)()"
    );
    run_lua(&lua)
}

/// Remove the process-independent Lua handle on a normal application exit.
pub fn remove_global_binding() {
    if is_hyprland() {
        let _ = run_lua(
            "(function() if _G.arcadeBoxGlobalShortcut then _G.arcadeBoxGlobalShortcut:remove(); _G.arcadeBoxGlobalShortcut = nil end; return \"ok\" end)()",
        );
    }
}

/// Configure the surface before GTK maps it. No tiled frame, compositor blur,
/// border, or compositor animation may appear around the transparent WebView.
/// These rules exist only in the compositor's current process, never its config.
pub fn install_surface_rules() -> Result<(), String> {
    if !is_hyprland() {
        return Ok(());
    }
    run_lua(
        r#"if _G.arcadeBoxSurfaceRule then
        _G.arcadeBoxSurfaceRule:set_enabled(true)
    else
        _G.arcadeBoxSurfaceRule = hl.window_rule({
            name = "arcade-box-island",
            match = { class = "^arcade-desktop$", title = "^Arcade Box$" },
            float = true, pin = true,
            move = {"(monitor_w-window_w)/2", 0},
            no_blur = true, no_shadow = true, no_dim = true, no_anim = true,
            decorate = false, border_size = 0, rounding = 0,
            opacity = "1 override 1 override 1 override"
        })
    end"#,
    )
}

pub fn remove_surface_rules() {
    if is_hyprland() {
        let _ = run_lua(
            "if _G.arcadeBoxSurfaceRule then _G.arcadeBoxSurfaceRule:set_enabled(false) end",
        );
    }
}

/// The compositor reapplies user config on reload, discarding temporary binds.
/// Listen for that event instead of polling the compositor while idle.
pub fn watch_reload(on_reload: impl Fn() + Send + 'static) {
    if !is_hyprland() {
        return;
    }
    let (Some(runtime), Some(signature)) = (
        env::var_os("XDG_RUNTIME_DIR"),
        env::var_os("HYPRLAND_INSTANCE_SIGNATURE"),
    ) else {
        return;
    };
    let path = std::path::PathBuf::from(runtime)
        .join("hypr")
        .join(signature)
        .join(".socket2.sock");
    std::thread::spawn(move || {
        use std::io::BufRead;
        let Ok(socket) = std::os::unix::net::UnixStream::connect(path) else {
            return;
        };
        for line in std::io::BufReader::new(socket).lines() {
            let Ok(line) = line else {
                break;
            };
            if line.starts_with("configreloaded>>") {
                on_reload();
            }
        }
    });
}

/// Resize an already mapped surface when switching between the tool island and
/// the explicit catalog. Initial positioning is handled by the pre-map rule.
pub fn place_island(width: f64, height: f64, centered: bool) -> bool {
    if !is_hyprland() {
        return false;
    }
    let process_id = std::process::id();
    let clients = hyprctl(&["-j", "clients"])
        .ok()
        .and_then(|output| serde_json::from_slice::<Value>(&output).ok());
    let client = clients
        .as_ref()
        .and_then(Value::as_array)
        .and_then(|clients| {
            clients.iter().find(|client| {
                client.get("pid").and_then(Value::as_u64) == Some(u64::from(process_id))
                    && client.get("title").and_then(Value::as_str) == Some("Arcade Box")
            })
        });
    let Some(client) = client else {
        return false;
    };
    let Some(address) = client.get("address").and_then(Value::as_str) else {
        return false;
    };
    let Some(hex) = address.strip_prefix("0x") else {
        return false;
    };
    if hex.is_empty() || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return false;
    }
    let monitors = hyprctl(&["-j", "monitors"])
        .ok()
        .and_then(|output| serde_json::from_slice::<Value>(&output).ok());
    let monitor = monitors
        .as_ref()
        .and_then(Value::as_array)
        .and_then(|monitors| {
            monitors
                .iter()
                .find(|monitor| monitor.get("id") == client.get("monitor"))
        });
    let Some(monitor) = monitor else {
        return false;
    };
    let (x, y, width, height) = surface_bounds(monitor, width, height, centered);
    let selector = format!("address:{address}");
    run_lua(&format!(r#"local w=hl.get_window("{selector}");
        if w and w.pid == {process_id} and w.title == "Arcade Box" then
            hl.dispatch(hl.dsp.window.resize({{x={width},y={height},relative=false,window="{selector}"}}))
            hl.dispatch(hl.dsp.window.move({{x={x},y={y},relative=false,window="{selector}"}}))
        end"#)).is_ok()
}

fn surface_bounds(
    monitor: &Value,
    width: f64,
    height: f64,
    centered: bool,
) -> (i64, i64, i64, i64) {
    let scale = monitor["scale"].as_f64().unwrap_or(1.0).max(0.25);
    let mut screen_width = monitor["width"].as_f64().unwrap_or(width);
    let mut screen_height = monitor["height"].as_f64().unwrap_or(height);
    if monitor["transform"].as_i64().unwrap_or(0) % 2 != 0 {
        std::mem::swap(&mut screen_width, &mut screen_height);
    }
    let screen_width = (screen_width / scale).round() as i64;
    let screen_height = (screen_height / scale).round() as i64;
    // Hyprland's reserved margins are left, top, right, bottom (logical pixels).
    let margin = |index| monitor["reserved"][index].as_i64().unwrap_or(0).max(0);
    let (left, top, right, bottom) = if centered {
        (margin(0), margin(1), margin(2), margin(3))
    } else {
        (0, 0, 0, 0)
    };
    let work_width = (screen_width - left - right).max(1);
    let work_height = (screen_height - top - bottom).max(1);
    let width = (width.round() as i64).clamp(1, work_width);
    let height = (height.round() as i64).clamp(1, work_height);
    let x = monitor["x"].as_i64().unwrap_or(0) + left + (work_width - width) / 2;
    let y = monitor["y"].as_i64().unwrap_or(0)
        + if centered {
            top + (work_height - height) / 2
        } else {
            0
        };
    (x, y, width, height)
}

fn reject_shortcut_conflict(key: &str, mask: i64) -> Result<(), String> {
    let output = hyprctl(&["-j", "binds"])?;
    let binds: Value = serde_json::from_slice(&output)
        .map_err(|error| format!("Could not inspect Hyprland shortcuts: {error}"))?;
    if binds.as_array().is_some_and(|binds| {
        binds.iter().any(|bind| {
            bind.get("key")
                .and_then(Value::as_str)
                .is_some_and(|bound| bound.eq_ignore_ascii_case(key))
                && bind.get("modmask").and_then(Value::as_i64) == Some(mask)
                && bind.get("description").and_then(Value::as_str) != Some("Arcade Box")
        })
    }) {
        Err("The selected shortcut is already in use by Hyprland. Choose another trigger.".into())
    } else {
        Ok(())
    }
}

fn shortcut_chord(trigger: &str) -> Result<(String, i64, String), String> {
    let mut parts = trigger.split('+').map(str::trim).collect::<Vec<_>>();
    let key = parts
        .pop()
        .filter(|key| !key.is_empty())
        .ok_or("Hyprland did not report a usable shortcut trigger")?;
    let mut mask = 0_i64;
    let mut modifiers = Vec::with_capacity(parts.len());
    for modifier in parts {
        let (name, bit) = match modifier.to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" => ("CTRL", 4),
            "ALT" => ("ALT", 8),
            "SHIFT" => ("SHIFT", 1),
            "LOGO" | "SUPER" | "META" => ("SUPER", 64),
            "NUM" | "MOD2" => ("MOD2", 16),
            _ => return Err("Hyprland received an unsupported shortcut modifier".into()),
        };
        mask |= bit;
        modifiers.push(name);
    }
    if mask == 0 {
        return Err("A global shortcut needs at least one modifier key".into());
    }
    let key = match key.to_ascii_uppercase().as_str() {
        "SPACE" => "SPACE".to_owned(),
        "RETURN" | "ENTER" => "RETURN".to_owned(),
        "ESC" | "ESCAPE" => "ESC".to_owned(),
        "BACKSPACE" => "BACKSPACE".to_owned(),
        "TAB" => "TAB".to_owned(),
        "PAGEUP" | "PAGE_UP" => "PAGE_UP".to_owned(),
        "PAGEDOWN" | "PAGE_DOWN" => "PAGE_DOWN".to_owned(),
        other
            if other
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') =>
        {
            other.to_owned()
        }
        _ => return Err("Hyprland received an unsupported shortcut key".into()),
    };
    modifiers.push(&key);
    Ok((modifiers.join(" + "), mask, key))
}

// Hyprland's local IPC avoids a subprocess on every surface change. Bound
// reads/writes so a busy compositor cannot indefinitely stall the UI thread.
fn hyprctl(arguments: &[&str]) -> Result<Vec<u8>, String> {
    use std::io::{Read, Write};
    let runtime = env::var_os("XDG_RUNTIME_DIR").ok_or("No desktop runtime directory")?;
    let signature = env::var_os("HYPRLAND_INSTANCE_SIGNATURE").ok_or("No Hyprland session")?;
    let path = std::path::PathBuf::from(runtime)
        .join("hypr")
        .join(signature)
        .join(".socket.sock");
    let mut socket =
        std::os::unix::net::UnixStream::connect(path).map_err(|error| error.to_string())?;
    let timeout = Some(std::time::Duration::from_millis(600));
    socket
        .set_read_timeout(timeout)
        .map_err(|error| error.to_string())?;
    socket
        .set_write_timeout(timeout)
        .map_err(|error| error.to_string())?;
    let (prefix, args) = if arguments.first() == Some(&"-j") {
        ("j/", &arguments[1..])
    } else {
        ("/", arguments)
    };
    let command = format!("{prefix}{}", args.join(" "));
    socket
        .write_all(command.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    socket
        .take(4 * 1024 * 1024)
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    if response.len() >= 4 * 1024 * 1024 {
        return Err("Hyprland IPC response exceeded its limit".into());
    }
    Ok(response)
}

fn run_lua(lua: &str) -> Result<(), String> {
    let output = hyprctl(&["eval", lua])?;
    let response = String::from_utf8_lossy(&output);
    if response.trim() == "ok" {
        Ok(())
    } else {
        Err(format!(
            "Hyprland rejected Arcade Box's temporary surface configuration: {}",
            response.trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scaled_offset_monitor_positions_in_logical_pixels() {
        let monitor =
            json!({"width":3840,"height":2160,"scale":2.0,"x":-1920,"y":0,"reserved":[0,30,0,0]});
        assert_eq!(
            surface_bounds(&monitor, 740.0, 740.0, false),
            (-1330, 0, 740, 740)
        );
        assert_eq!(
            surface_bounds(&monitor, 1100.0, 760.0, true),
            (-1510, 175, 1100, 760)
        );
    }

    #[test]
    fn rotated_small_monitor_clamps_surface() {
        let monitor = json!({"width":1920,"height":1080,"scale":2.0,"transform":1,"x":0,"y":0});
        assert_eq!(
            surface_bounds(&monitor, 740.0, 740.0, false),
            (0, 0, 540, 740)
        );
    }

    #[test]
    fn canonical_chord_and_unsafe_keys() {
        assert_eq!(
            shortcut_chord("CTRL+ALT+space").unwrap(),
            ("CTRL + ALT + SPACE".into(), 12, "SPACE".into())
        );
        assert!(shortcut_chord("CTRL+space\";oops").is_err());
        assert!(shortcut_chord("space").is_err());
    }
}
