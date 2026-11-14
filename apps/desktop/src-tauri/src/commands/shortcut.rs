//! Box's global shortcut: the native binding (or the Wayland portal's), its status and changes from the UI or Link.

use crate::*;

#[derive(Clone)]
pub(crate) struct NativeShortcutController {
    state: Arc<Mutex<NativeShortcutState>>,
}

pub(crate) struct NativeShortcutState {
    current: Option<Shortcut>,
    message: String,
}

#[tauri::command]
pub(crate) fn shortcut_status(app: tauri::AppHandle) -> serde_json::Value {
    #[cfg(target_os = "linux")]
    if let Some(controller) = app.try_state::<wayland_shortcut::WaylandShortcutController>() {
        return serde_json::to_value(controller.status()).unwrap_or_default();
    }
    let Some(controller) = app.try_state::<NativeShortcutController>() else {
        return serde_json::json!({"backend":"native","state":"unavailable","message":"Shortcut service is unavailable","trigger_description":null});
    };
    let state = controller
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    serde_json::json!({
        "backend":"native",
        "state": if state.current.is_some() { "registered" } else { "unavailable" },
        "message":state.message,
        "trigger_description":state.current.as_ref().map(shortcut_label)
    })
}

/// A shortcut as people write it (`Ctrl+Alt+B`, not `control+alt+KeyB`),
/// in a form that also parses back.
pub(crate) fn shortcut_label(shortcut: &Shortcut) -> String {
    let mac = cfg!(target_os = "macos");
    let mut parts: Vec<String> = [
        (Modifiers::CONTROL, "Ctrl"),
        (Modifiers::ALT, if mac { "Option" } else { "Alt" }),
        (Modifiers::SHIFT, "Shift"),
        (Modifiers::SUPER, if mac { "Cmd" } else { "Super" }),
    ]
    .into_iter()
    .filter(|(flag, _)| shortcut.mods.contains(*flag))
    .map(|(_, name)| name.to_owned())
    .collect();
    let key = shortcut.key.to_string();
    parts.push(
        key.strip_prefix("Key")
            .or_else(|| key.strip_prefix("Digit"))
            .unwrap_or(&key)
            .to_owned(),
    );
    parts.join("+")
}

#[tauri::command]
pub(crate) fn set_shortcut(
    trigger: String,
    app: tauri::AppHandle,
    runtime: tauri::State<'_, Arc<Arcade>>,
) -> Result<serde_json::Value, String> {
    let trigger = shortcut_syntax::canonical_trigger(&trigger)?;
    let _change = link::settings_change_lock();
    let previous = runtime
        .storage()
        .setting("global_shortcut")
        .ok()
        .flatten()
        .and_then(|saved| shortcut_syntax::canonical_trigger(&saved).ok())
        .unwrap_or_else(|| shortcut_syntax::DEFAULT.into());
    let result = apply_shortcut_on_ui(&app, &trigger)?;
    if let Err(error) = runtime.storage().set_setting("global_shortcut", &trigger) {
        let _ = apply_shortcut_on_ui(&app, &previous);
        return Err(error.to_string());
    }
    if let Err(error) = link::publish_settings(&runtime) {
        let _ = runtime.storage().set_setting("global_shortcut", &previous);
        let _ = apply_shortcut_on_ui(&app, &previous);
        let _ = link::publish_settings(&runtime);
        return Err(error.to_string());
    }
    let status = shortcut_status(app.clone());
    let _ = app.emit("arcade://shortcut-status", status.clone());
    let _ = result;
    Ok(status)
}

pub(crate) fn manual_shortcut(hint: String) -> arcade_link::app::ShortcutSetResult {
    arcade_link::app::ShortcutSetResult {
        applied: false,
        via: arcade_link::app::ShortcutVia::Manual,
        hint: Some(hint),
    }
}

pub(crate) fn apply_shortcut_on_ui(
    app: &tauri::AppHandle,
    canonical: &str,
) -> Result<arcade_link::app::ShortcutSetResult, String> {
    #[cfg(target_os = "linux")]
    if let Some(controller) = app.try_state::<wayland_shortcut::WaylandShortcutController>() {
        if canonical.contains(' ') {
            return Ok(manual_shortcut(String::from(
                "Bind `arcade-desktop --toggle` in your desktop's keyboard settings; this portal accepts one chord.",
            )));
        }
        let trigger = match shortcut_syntax::xdg_trigger(canonical) {
            Ok(trigger) => trigger,
            Err(error) => {
                return Ok(manual_shortcut(format!(
                    "{error}. Bind `arcade-desktop --toggle` in your desktop's keyboard settings."
                )));
            }
        };
        controller.set_preferred_trigger(trigger)?;
        return Ok(arcade_link::app::ShortcutSetResult {
            applied: true,
            via: arcade_link::app::ShortcutVia::Portal,
            hint: None,
        });
    }
    if canonical.contains(' ') {
        return Ok(manual_shortcut(
            "Bind `arcade-desktop --toggle` in your desktop's keyboard settings; native registration accepts one chord.".into(),
        ));
    }
    let shortcut = match canonical.parse::<Shortcut>() {
        Ok(shortcut) if !shortcut.mods.is_empty() => shortcut,
        _ => {
            return Ok(manual_shortcut(
                "Bind `arcade-desktop --toggle` in your desktop's keyboard settings.".into(),
            ));
        }
    };
    let Some(controller) = app.try_state::<NativeShortcutController>() else {
        return Ok(manual_shortcut(
            "Bind `arcade-desktop --toggle` in your desktop's keyboard settings.".into(),
        ));
    };
    let mut state = controller
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if state.current != Some(shortcut) {
        if let Err(error) = app.global_shortcut().register(shortcut) {
            state.message = format!("Shortcut needs manual registration: {error}");
            return Ok(manual_shortcut(
                "Bind `arcade-desktop --toggle` in your desktop's keyboard settings.".into(),
            ));
        }
        if let Some(previous) = state.current {
            let _ = app.global_shortcut().unregister(previous);
        }
        state.current = Some(shortcut);
    }
    state.message = "Arcade Box global shortcut is registered".into();
    Ok(arcade_link::app::ShortcutSetResult {
        applied: true,
        via: arcade_link::app::ShortcutVia::Native,
        hint: None,
    })
}

pub(crate) fn set_shortcut_from_link(
    app: &tauri::AppHandle,
    accelerator: String,
) -> Result<arcade_link::app::ShortcutSetResult, arcade_link::LinkError> {
    let canonical = arcade_link::accelerator::normalize(&accelerator)
        .map_err(|error| arcade_link::LinkError::denied(&error.to_string()))?;
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let app_handle = app.clone();
    app.run_on_main_thread(move || {
        let result = apply_shortcut_on_ui(&app_handle, &canonical)
            .map_err(arcade_link::LinkError::unavailable);
        let _ = send.send(result);
    })
    .map_err(|error| arcade_link::LinkError::unavailable(error.to_string()))?;
    receive
        .recv()
        .map_err(|error| arcade_link::LinkError::unavailable(error.to_string()))?
}

pub(crate) fn register_native_shortcut(
    app: &tauri::AppHandle,
    shortcut: Shortcut,
) -> tauri::Result<NativeShortcutController> {
    let controller = NativeShortcutController {
        state: Arc::new(Mutex::new(NativeShortcutState {
            current: None,
            message: "Checking the global shortcut".into(),
        })),
    };
    let current = controller.state.clone();
    app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |handle, pressed, event| {
                let active = current
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .current;
                if Some(*pressed) == active && event.state() == ShortcutState::Pressed {
                    toggle_island_app(handle);
                }
            })
            .build(),
    )?;
    let mut state = controller
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match app.global_shortcut().register(shortcut) {
        Ok(()) => {
            state.current = Some(shortcut);
            state.message = "Arcade Box global shortcut is registered".into();
        }
        Err(error) => state.message = format!("Shortcut unavailable: {error}"),
    }
    drop(state);
    Ok(controller)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_labels_are_readable_and_parse_back() {
        for (shortcut, label) in [
            (
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyB),
                "Ctrl+Alt+B",
            ),
            (
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Digit7),
                "Ctrl+Shift+7",
            ),
            (
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space),
                "Ctrl+Alt+Space",
            ),
        ] {
            if cfg!(not(target_os = "macos")) {
                assert_eq!(shortcut_label(&shortcut), label);
            }
            assert_eq!(
                shortcut_label(&shortcut).parse::<Shortcut>().unwrap(),
                shortcut
            );
        }
    }
}
