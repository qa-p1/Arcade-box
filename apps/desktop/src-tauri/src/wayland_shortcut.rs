//! Wayland global shortcut support through the XDG Desktop Portal.
//!
//! Keep this adapter separate from the Tauri global-shortcut plugin: that
//! plugin's Linux backend is X11-only. The portal may be absent on some
//! desktops, so registration outcomes are emitted for the UI to explain.

use ashpd::desktop::CreateSessionOptions;
use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures_util::{FutureExt, StreamExt};
use serde::Serialize;
use std::{
    sync::{Arc, RwLock},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};

const SHORTCUT_ID: &str = "arcade-box-toggle";
const SHORTCUT_DESCRIPTION: &str = "Show or hide Arcade Box";
pub const STATUS_EVENT: &str = "arcade://shortcut-status";
pub const ACTIVATED_EVENT: &str = "arcade://shortcut-activated";

/// Wayland can be reported by either variable, depending on how the app was
/// launched. Prefer the display socket but retain the session type fallback.
pub fn is_wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|display| !display.is_empty())
        || std::env::var("XDG_SESSION_TYPE")
            .is_ok_and(|session| session.eq_ignore_ascii_case("wayland"))
}

/// Register this process with the host portal before any other ashpd portal
/// call is made. The portal registry expects this application's desktop-file
/// ID, which matches Tauri's configured application identifier.
pub fn register_host_app(app: &AppHandle) -> Result<(), String> {
    let identifier = app.config().identifier.as_str();
    let app_id = ashpd::AppID::try_from(identifier)
        .map_err(|error| format!("Invalid portal application ID `{identifier}`: {error}"))?;

    tauri::async_runtime::block_on(async move {
        tokio::time::timeout(
            Duration::from_secs(3),
            ashpd::register_host_app(app_id),
        )
        .await
        .map_err(|_| "The desktop portal did not answer while registering Arcade Box".to_owned())?
        .map_err(|error| {
            format!(
                "Could not register portal identity `{identifier}`. Ensure `{identifier}.desktop` is installed before launching: {error}"
            )
        })
    })
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ShortcutStatus {
    pub backend: &'static str,
    pub state: &'static str,
    pub message: String,
    pub trigger_description: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShortcutActivation {
    pub shortcut_id: &'static str,
    /// A short-lived portal activation token, when the backend provides one.
    /// Do not persist or log this value.
    pub activation_token: Option<String>,
}

/// Cloneable in-process state for Tauri command handlers and the settings UI.
///
/// The status snapshot is retained so a frontend that mounts after startup
/// does not need to have observed the initial event.
#[derive(Clone)]
pub struct WaylandShortcutController {
    app: AppHandle,
    status: Arc<RwLock<ShortcutStatus>>,
    updates: UnboundedSender<String>,
    identity_error: Option<String>,
    preferred: Arc<RwLock<String>>,
}

impl WaylandShortcutController {
    /// Get the latest capability and registration state.
    pub fn status(&self) -> ShortcutStatus {
        self.status
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Ask the desktop portal to register a new preferred trigger.
    ///
    /// The desktop may replace the preferred trigger or decline registration.
    pub fn set_preferred_trigger(&self, preferred_trigger: String) -> Result<(), String> {
        validate_trigger(&preferred_trigger)?;
        *self
            .preferred
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = preferred_trigger.clone();
        self.publish_status("updating", "Applying the preferred global shortcut", None);
        if self.updates.unbounded_send(preferred_trigger).is_err() {
            let message = "The global shortcut worker is no longer running";
            self.publish_status("unavailable", message, None);
            return Err(message.to_owned());
        }
        Ok(())
    }

    fn publish_status(
        &self,
        state: &'static str,
        message: impl Into<String>,
        trigger_description: Option<String>,
    ) {
        let message = message.into();
        eprintln!("Arcade Box shortcut status: state={state} message={message}");
        let status = ShortcutStatus {
            backend: "xdg-desktop-portal",
            state,
            message,
            trigger_description,
        };
        *self
            .status
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = status.clone();
        let _ = self.app.emit(STATUS_EVENT, status);
    }
}

/// Start asynchronous Wayland registration and event handling.
///
/// The preferred trigger uses the XDG Shortcuts format, for example
/// CTRL+ALT+space. The desktop may choose a different trigger or decline it.
/// Keep the returned controller in Tauri managed state for status queries and
/// first-run shortcut customization.
pub fn start(
    app: AppHandle,
    preferred_trigger: String,
    identity_result: Result<(), String>,
) -> WaylandShortcutController {
    let (updates, receiver) = futures_channel::mpsc::unbounded();
    let controller = WaylandShortcutController {
        app,
        status: Arc::new(RwLock::new(ShortcutStatus {
            backend: "xdg-desktop-portal",
            state: "starting",
            message: "Global shortcut registration is starting".to_owned(),
            trigger_description: None,
        })),
        updates,
        identity_error: identity_result.err(),
        preferred: Arc::new(RwLock::new(preferred_trigger.clone())),
    };
    let reconnect = controller.clone();
    crate::hyprland::watch_reload(move || {
        let preferred = reconnect
            .preferred
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let _ = reconnect.set_preferred_trigger(preferred);
    });
    let worker = controller.clone();
    tauri::async_runtime::spawn(async move {
        run_controller(worker, preferred_trigger, receiver).await;
    });
    controller
}

fn validate_trigger(trigger: &str) -> Result<(), String> {
    if trigger.is_empty() {
        return Ok(());
    }
    if trigger.len() > 96 {
        return Err("Shortcut trigger is too long".into());
    }
    let parts: Vec<_> = trigger.split('+').collect();
    let valid_identifier = |part: &str| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    };
    let modifiers = ["CTRL", "ALT", "SHIFT", "NUM", "LOGO"];
    let valid = parts.iter().all(|part| valid_identifier(part))
        && parts[..parts.len().saturating_sub(1)]
            .iter()
            .all(|part| modifiers.contains(part));
    if valid {
        Ok(())
    } else {
        Err("Use XDG shortcut syntax such as CTRL+ALT+space".into())
    }
}

async fn run_controller(
    controller: WaylandShortcutController,
    mut preferred_trigger: String,
    mut updates: UnboundedReceiver<String>,
) {
    loop {
        if let Err(error) = validate_trigger(&preferred_trigger) {
            controller.publish_status("invalid_trigger", error, None);
            match updates.next().await {
                Some(new_trigger) => preferred_trigger = new_trigger,
                None => return,
            }
            continue;
        }
        match register_and_listen(&controller, &preferred_trigger, &mut updates).await {
            Some(new_trigger) => preferred_trigger = new_trigger,
            None => match updates.next().await {
                Some(new_trigger) => preferred_trigger = new_trigger,
                None => return,
            },
        }
    }
}

async fn register_and_listen(
    controller: &WaylandShortcutController,
    preferred_trigger: &str,
    updates: &mut UnboundedReceiver<String>,
) -> Option<String> {
    crate::hyprland::remove_global_binding();
    if let Some(error) = &controller.identity_error {
        controller.publish_status("identity_unavailable", error.clone(), None);
        return None;
    }

    controller.publish_status(
        "checking",
        "Checking the desktop's global shortcut portal",
        None,
    );

    let portal = match GlobalShortcuts::new().await {
        Ok(portal) => portal,
        Err(ashpd::Error::PortalNotFound(_)) => {
            controller.publish_status(
                "unsupported",
                "This desktop does not expose the XDG GlobalShortcuts portal",
                None,
            );
            return None;
        }
        Err(error) => {
            controller.publish_status(
                "unavailable",
                format!("Could not connect to the XDG GlobalShortcuts portal: {error}"),
                None,
            );
            return None;
        }
    };
    let mut activated = match portal.receive_activated().await {
        Ok(stream) => stream,
        Err(error) => {
            controller.publish_status(
                "error",
                format!("Could not listen for portal shortcut events: {error}"),
                None,
            );
            return None;
        }
    };

    let mut deactivated = match portal.receive_deactivated().await {
        Ok(stream) => stream,
        Err(error) => {
            controller.publish_status(
                "error",
                format!("Could not listen for shortcut releases: {error}"),
                None,
            );
            return None;
        }
    };

    let session = match portal.create_session(CreateSessionOptions::default()).await {
        Ok(session) => session,
        Err(error) => {
            controller.publish_status(
                "error",
                format!("Could not create a shortcut session: {error}"),
                None,
            );
            return None;
        }
    };

    // Session implements Serialize as its public D-Bus object path.
    let session_handle = serde_json::to_value(&session).ok();
    let mut shortcut_down = false;

    let mut shortcut = NewShortcut::new(SHORTCUT_ID, SHORTCUT_DESCRIPTION);
    if !preferred_trigger.trim().is_empty() {
        shortcut = shortcut.preferred_trigger(Some(preferred_trigger));
    }

    let request = match portal
        .bind_shortcuts(&session, &[shortcut], None, BindShortcutsOptions::default())
        .await
    {
        Ok(request) => request,
        Err(error) => {
            let _ = session.close().await;
            controller.publish_status(
                "error",
                format!("Could not request a global shortcut: {error}"),
                None,
            );
            return None;
        }
    };

    let binding = match request.response() {
        Ok(binding) => binding,
        Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => {
            let _ = session.close().await;
            controller.publish_status(
                "user_declined",
                "Global shortcut registration was declined or cancelled",
                None,
            );
            return None;
        }
        Err(ashpd::Error::Response(_)) => {
            let _ = session.close().await;
            controller.publish_status(
                "registration_rejected",
                "The desktop could not complete global shortcut registration",
                None,
            );
            return None;
        }
        Err(error) => {
            let _ = session.close().await;
            controller.publish_status(
                "error",
                format!("The desktop could not register the global shortcut: {error}"),
                None,
            );
            return None;
        }
    };

    let Some(bound_shortcut) = binding
        .shortcuts()
        .iter()
        .find(|shortcut| shortcut.id() == SHORTCUT_ID)
    else {
        let _ = session.close().await;
        controller.publish_status(
            "not_bound",
            "The desktop did not bind the requested shortcut. Check the desktop shortcut settings or choose another shortcut.",
            None,
        );
        return None;
    };
    // Some portal backends bind the requested shortcut but omit its readable
    // trigger description in the response. Keep the user's preferred trigger
    // as the fallback so compositor adapters can still install the binding.
    let portal_description = bound_shortcut.trigger_description().trim();
    let trigger_description = Some(if portal_description.is_empty() {
        preferred_trigger.to_owned()
    } else {
        portal_description.to_owned()
    });

    if crate::hyprland::is_hyprland() {
        let app_id = controller.app.config().identifier.clone();
        // trigger_description is localized, user-facing text and Hyprland's
        // portal commonly leaves it blank. Use the validated canonical key
        // submitted to BindShortcuts for the temporary compositor binding.
        if let Err(error) =
            crate::hyprland::install_global_binding(&app_id, SHORTCUT_ID, preferred_trigger)
        {
            crate::hyprland::remove_global_binding();
            let _ = session.close().await;
            controller.publish_status("hyprland_binding_unavailable", error, trigger_description);
            return None;
        }
    }

    controller.publish_status(
        "registered",
        "Arcade Box global shortcut is registered",
        trigger_description,
    );

    loop {
        let next_activation = activated.next().fuse();
        let next_release = deactivated.next().fuse();
        let next_update = updates.next().fuse();
        futures_util::pin_mut!(next_activation, next_release, next_update);
        futures_util::select! {
            event = next_activation => match event {
                Some(event) if event.shortcut_id() == SHORTCUT_ID && Some(event.session_handle().as_str()) == session_handle.as_ref().and_then(serde_json::Value::as_str) => {
                    if shortcut_down { continue; }
                    shortcut_down = true;
                    let activation_token = event
                        .options()
                        .get("activation_token")
                        .and_then(|value| value.try_clone().ok())
                        .and_then(|value| String::try_from(value).ok());
                    let payload = ShortcutActivation {
                        shortcut_id: SHORTCUT_ID,
                        activation_token,
                    };

                    if let Some(window) = controller.app.get_webview_window("island") {
                        super::toggle_island(&window);
                        let _ = window.emit(ACTIVATED_EVENT, payload);
                    }
                }
                Some(_) => {}
                None => {
                    let _ = session.close().await;
                    crate::hyprland::remove_global_binding();
                    controller.publish_status(
                        "disconnected",
                        "The desktop shortcut event stream ended",
                        None,
                    );
                    return None;
                }
            },
            event = next_release => match event {
                Some(event) if event.shortcut_id() == SHORTCUT_ID
                    && Some(event.session_handle().as_str()) == session_handle.as_ref().and_then(serde_json::Value::as_str) => {
                    shortcut_down = false;
                }
                Some(_) => {}
                None => {
                    let _ = session.close().await;
                    crate::hyprland::remove_global_binding();
                    controller.publish_status("disconnected", "The desktop shortcut event stream ended", None);
                    return None;
                }
            },
            update = next_update => match update {
                Some(new_trigger) => {
                    let _ = session.close().await;
                    return Some(new_trigger);
                }
                None => {
                    let _ = session.close().await;
                    controller.publish_status(
                        "stopped",
                        "The global shortcut worker stopped",
                        None,
                    );
                    return None;
                }
            }
        }
    }
}
