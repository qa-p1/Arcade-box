//! The two spellings of a global shortcut.
//!
//! Box stores and shows Arcade Link's canonical chord (`Ctrl+Alt+Space`).
//! The XDG desktop portal takes the Shortcuts-spec trigger instead: upper-case
//! modifier names and an xkb keysym (`CTRL+ALT+space`). Every Wayland path
//! (startup, Settings, Link, a Hyprland reload) converts through
//! [`xdg_trigger`], and a saved value in either spelling is read back with
//! [`canonical_trigger`].

use arcade_link::accelerator;

/// The default chord on every OS (STANDARD §10: ⌃⌥Space on macOS too).
pub const DEFAULT: &str = "Ctrl+Alt+Space";
/// macOS's default before 0.4.0 (⇧⌘Space), which existing users keep.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const MACOS_PREVIOUS_DEFAULT: &str = "Shift+Super+Space";

/// A saved shortcut in either spelling, as the canonical chord. Builds before
/// 0.4.0 could leave the portal spelling in settings.
pub fn canonical_trigger(saved: &str) -> Result<String, String> {
    let chord = saved
        .trim()
        .split('+')
        .map(|part| match part.trim() {
            // xkb names Link's parser doesn't know.
            "Scroll_Lock" => "ScrollLock",
            "Caps_Lock" => "CapsLock",
            "Num_Lock" => "NumLock",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("+");
    accelerator::normalize(&chord).map_err(|error| error.to_string())
}

/// The canonical chord as the portal's trigger.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn xdg_trigger(chord: &str) -> Result<String, String> {
    let canonical = canonical_trigger(chord)?;
    if canonical.contains(' ') {
        return Err("The desktop portal takes one chord, not a sequence".into());
    }
    canonical
        .split('+')
        .map(|part| match part {
            "Ctrl" => Ok("CTRL".to_owned()),
            "Alt" => Ok("ALT".to_owned()),
            "Shift" => Ok("SHIFT".to_owned()),
            "Super" => Ok("LOGO".to_owned()),
            key => keysym(key).ok_or_else(|| format!("{key} can't be a desktop shortcut")),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("+"))
}

/// The xkb keysym for a canonical key, or None for mouse buttons, the wheel
/// and raw key codes, which the portal can't bind.
fn keysym(key: &str) -> Option<String> {
    let named = match key {
        "Space" => "space",
        "Enter" => "Return",
        "Tab" => "Tab",
        "Escape" => "Escape",
        "Backspace" => "BackSpace",
        "Delete" => "Delete",
        "Insert" => "Insert",
        "Home" => "Home",
        "End" => "End",
        "PageUp" => "Page_Up",
        "PageDown" => "Page_Down",
        "Up" => "Up",
        "Down" => "Down",
        "Left" => "Left",
        "Right" => "Right",
        "Minus" => "minus",
        "Equal" => "equal",
        "BracketLeft" => "bracketleft",
        "BracketRight" => "bracketright",
        "Backslash" => "backslash",
        "Semicolon" => "semicolon",
        "Quote" => "apostrophe",
        "Backquote" => "grave",
        "Comma" => "comma",
        "Period" => "period",
        "Slash" => "slash",
        "Print" => "Print",
        "Pause" => "Pause",
        "ScrollLock" => "Scroll_Lock",
        "CapsLock" => "Caps_Lock",
        "NumLock" => "Num_Lock",
        "Menu" => "Menu",
        "NumpadAdd" => "KP_Add",
        "NumpadSubtract" => "KP_Subtract",
        "NumpadMultiply" => "KP_Multiply",
        "NumpadDivide" => "KP_Divide",
        "NumpadDecimal" => "KP_Decimal",
        "NumpadEnter" => "KP_Enter",
        "VolumeUp" => "XF86AudioRaiseVolume",
        "VolumeDown" => "XF86AudioLowerVolume",
        "VolumeMute" => "XF86AudioMute",
        "MicMute" => "XF86AudioMicMute",
        "MediaPlayPause" => "XF86AudioPlay",
        "MediaNext" => "XF86AudioNext",
        "MediaPrevious" => "XF86AudioPrev",
        "MediaStop" => "XF86AudioStop",
        "BrightnessUp" => "XF86MonBrightnessUp",
        "BrightnessDown" => "XF86MonBrightnessDown",
        _ => "",
    };
    if !named.is_empty() {
        return Some(named.to_owned());
    }
    if let Some(digit) = key.strip_prefix("Numpad") {
        return Some(format!("KP_{digit}"));
    }
    if key.len() == 1 {
        return Some(key.to_ascii_lowercase());
    }
    if key
        .strip_prefix('F')
        .is_some_and(|n| n.parse::<u8>().is_ok())
    {
        return Some(key.to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> Vec<String> {
        let named = [
            "Space",
            "Enter",
            "Tab",
            "Escape",
            "Backspace",
            "Delete",
            "Insert",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "Up",
            "Down",
            "Left",
            "Right",
            "Minus",
            "Equal",
            "BracketLeft",
            "BracketRight",
            "Backslash",
            "Semicolon",
            "Quote",
            "Backquote",
            "Comma",
            "Period",
            "Slash",
            "Print",
            "Pause",
            "ScrollLock",
            "CapsLock",
            "NumLock",
            "Menu",
            "NumpadAdd",
            "NumpadSubtract",
            "NumpadMultiply",
            "NumpadDivide",
            "NumpadDecimal",
            "NumpadEnter",
            "VolumeUp",
            "VolumeDown",
            "VolumeMute",
            "MicMute",
            "MediaPlayPause",
            "MediaNext",
            "MediaPrevious",
            "MediaStop",
            "BrightnessUp",
            "BrightnessDown",
        ];
        let mut keys: Vec<String> = named.iter().map(|key| (*key).to_owned()).collect();
        keys.extend(('A'..='Z').chain('0'..='9').map(String::from));
        keys.extend((1..=24).map(|n| format!("F{n}")));
        keys.extend((0..=9).map(|n| format!("Numpad{n}")));
        keys
    }

    #[test]
    fn every_modifier_and_key_round_trips_through_the_portal_spelling() {
        let modifiers = [
            "",
            "Ctrl+",
            "Alt+",
            "Shift+",
            "Super+",
            "Ctrl+Alt+",
            "Ctrl+Alt+Shift+Super+",
        ];
        for key in keys() {
            for modifier in modifiers {
                let canonical = format!("{modifier}{key}");
                let xdg =
                    xdg_trigger(&canonical).unwrap_or_else(|error| panic!("{canonical}: {error}"));
                assert!(
                    xdg.bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'+'),
                    "{canonical} -> {xdg}"
                );
                assert_eq!(
                    canonical_trigger(&xdg).as_deref(),
                    Ok(canonical.as_str()),
                    "{xdg}"
                );
            }
        }
    }

    #[test]
    fn the_saved_default_reaches_the_portal_in_its_syntax() {
        // B1: 0.3.5 handed the saved `Ctrl+Alt+Space` to the portal as is.
        assert_eq!(
            xdg_trigger("Ctrl+Alt+Space").as_deref(),
            Ok("CTRL+ALT+space")
        );
        assert_eq!(
            xdg_trigger("Ctrl+Shift+Enter").as_deref(),
            Ok("CTRL+SHIFT+Return")
        );
        assert_eq!(xdg_trigger("Super+PageUp").as_deref(), Ok("LOGO+Page_Up"));
    }

    #[test]
    fn either_saved_spelling_reads_back_as_the_canonical_chord() {
        for saved in ["Ctrl+Alt+Space", "CTRL+ALT+space", " ctrl + alt + space "] {
            assert_eq!(
                canonical_trigger(saved).as_deref(),
                Ok("Ctrl+Alt+Space"),
                "{saved}"
            );
        }
        assert_eq!(
            canonical_trigger("LOGO+SHIFT+Scroll_Lock").as_deref(),
            Ok("Shift+Super+ScrollLock")
        );
    }

    #[test]
    fn the_portal_refuses_what_it_cannot_bind() {
        assert!(xdg_trigger("Ctrl+K Ctrl+S").is_err());
        assert!(xdg_trigger("Ctrl+MouseLeft").is_err());
        assert!(xdg_trigger("").is_err());
    }
}
