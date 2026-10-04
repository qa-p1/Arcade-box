//! Arcade Link: Box's presence among the other Arcade apps.
//!
//! The resident desktop process writes Box's manifest and listens on its
//! endpoint from a background thread after setup, so startup never waits.
//! With "Connect with other Arcade apps" off, the manifest has no actions
//! and nothing listens.

use std::sync::{Arc, Mutex, OnceLock};

use arcade_core::Arcade;
use arcade_core::link::{LinkSettings, manifest};
use arcade_link::server::{Handler, InvokeContext, Reply};
use arcade_link::{Action, InvokeRequest, LinkError, Locations, Manifest, Presence};
use tauri::{AppHandle, Manager};

/// The shortcut Box uses now, for clash warnings in other apps.
pub fn effective_shortcut(runtime: &Arcade) -> String {
    runtime
        .storage()
        .setting("global_shortcut")
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                "Cmd+Shift+Space".into()
            } else {
                "Ctrl+Alt+Space".into()
            }
        })
}

/// The data directory the desktop app uses (Tauri's `app_data_dir`), for
/// code that runs before Tauri starts (`--arcade-manifest`, one-shot mode).
pub fn data_dir() -> Option<std::path::PathBuf> {
    directories::BaseDirs::new().map(|d| d.data_dir().join("dev.arcadebox.app"))
}

fn build(runtime: &Arcade) -> Manifest {
    manifest(
        &LinkSettings::load(runtime),
        &arcade_link::manifest::current_executable(),
        env!("CARGO_PKG_VERSION"),
        Some(&effective_shortcut(runtime)),
    )
}

/// `--arcade-manifest`: the manifest from the saved settings. Doesn't create
/// Box's database if it doesn't exist yet.
pub fn print_manifest() {
    let db = data_dir().map(|d| d.join("arcade.sqlite3"));
    let m = match db
        .filter(|p| p.is_file())
        .and_then(|p| Arcade::open(&p).ok())
    {
        Some(runtime) => build(&runtime),
        None => manifest(
            &LinkSettings::default(),
            &arcade_link::manifest::current_executable(),
            env!("CARGO_PKG_VERSION"),
            Some(if cfg!(target_os = "macos") {
                "Cmd+Shift+Space"
            } else {
                "Ctrl+Alt+Space"
            }),
        ),
    };
    println!("{}", m.to_json());
}

struct BoxHandler {
    #[allow(dead_code)]
    app: AppHandle,
}

impl Handler for BoxHandler {
    fn describe(&self) -> Vec<Action> {
        Vec::new()
    }

    fn invoke(&self, request: InvokeRequest, _ctx: &InvokeContext) -> Result<Reply, LinkError> {
        Err(LinkError::unavailable(format!(
            "Arcade Box has no action {}",
            request.action
        )))
    }
}

static PRESENCE: OnceLock<Mutex<Option<Arc<Presence>>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Arc<Presence>>> {
    PRESENCE.get_or_init(|| Mutex::new(None))
}

/// Starts Box's presence on a background thread.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("box-link".into())
        .spawn(move || {
            let Some(runtime) = app.try_state::<Arc<Arcade>>().map(|r| r.inner().clone()) else {
                return;
            };
            let p = Presence::start(
                Locations::discover(),
                build(&runtime),
                Arc::new(BoxHandler { app: app.clone() }),
            );
            if let Some(e) = p.last_error() {
                eprintln!("Arcade Link: {e}");
            }
            *slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(p));
        });
}

/// Rewrites the manifest after a change (shortcut, Link settings, providers).
pub fn refresh(app: &AppHandle) {
    let Some(runtime) = app.try_state::<Arc<Arcade>>().map(|r| r.inner().clone()) else {
        return;
    };
    let p = slot().lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(p) = p {
        std::thread::spawn(move || p.update(build(&runtime)));
    }
}

/// Stops listening and removes the endpoint file (the manifest stays).
pub fn stop() {
    if let Some(p) = slot().lock().unwrap_or_else(|e| e.into_inner()).take() {
        p.stop();
    }
}
