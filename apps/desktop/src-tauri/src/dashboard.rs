//! The dashboard: a normal decorated window with a taskbar entry, created on
//! demand from its `tauri.conf.json` entry (`"create": false`). Closing it hides
//! it, and its WebView is destroyed 15 s later unless it is opened again, the
//! same memory policy as the Island.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tauri::{Emitter, Manager};

pub(crate) const LABEL: &str = "dashboard";
const RELEASE_AFTER_CLOSE: Duration = Duration::from_secs(15);
/// The pages `open` accepts; anything else opens the dashboard where it was.
pub(crate) const PAGES: [&str; 8] = [
    "all",
    "favorites",
    "recent",
    "jobs",
    "pipelines",
    "plugins",
    "engines",
    "settings",
];

/// True once the dashboard's page has registered its listeners.
static READY: AtomicBool = AtomicBool::new(false);
/// Bumped on every open, so a release timer from an earlier close gives way.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// The page asked for before the WebView was ready.
static PENDING_PAGE: Mutex<Option<String>> = Mutex::new(None);

/// Opens (or focuses) the dashboard, on `page` when one is given.
pub(crate) fn open(app: &tauri::AppHandle, page: Option<&str>) {
    let page = page.filter(|page| PAGES.contains(page)).map(str::to_owned);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(window) = window(&handle) else {
            return;
        };
        GENERATION.fetch_add(1, Ordering::SeqCst);
        if let Some(page) = page {
            if READY.load(Ordering::SeqCst) {
                let _ = window.emit_to(LABEL, "arcade://dashboard-page", page);
            } else {
                *PENDING_PAGE
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(page);
            }
        }
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    });
}

/// The existing dashboard, or a new one. Must run on the main thread.
fn window(app: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Some(window);
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == LABEL)?
        .clone();
    READY.store(false, Ordering::SeqCst);
    let window = match tauri::WebviewWindowBuilder::from_config(app, &config)
        .and_then(tauri::WebviewWindowBuilder::build)
    {
        Ok(window) => window,
        Err(error) => {
            eprintln!("Could not create the Arcade Box dashboard: {error}");
            return None;
        }
    };
    let closing = window.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = closing.hide();
            schedule_release(&closing);
        }
        tauri::WindowEvent::Destroyed => READY.store(false, Ordering::SeqCst),
        _ => {}
    });
    Some(window)
}

/// Destroys the hidden dashboard after `RELEASE_AFTER_CLOSE` unless it was
/// opened again in the meantime.
fn schedule_release(window: &tauri::WebviewWindow) {
    let generation = GENERATION.load(Ordering::SeqCst);
    let window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(RELEASE_AFTER_CLOSE);
        if GENERATION.load(Ordering::SeqCst) != generation || window.is_visible().unwrap_or(false) {
            return;
        }
        let target = window.clone();
        let _ = window.run_on_main_thread(move || {
            if GENERATION.load(Ordering::SeqCst) == generation {
                let _ = target.destroy();
            }
        });
    });
}

/// The dashboard page is listening: deliver a page asked for before it loaded.
#[tauri::command]
pub(crate) fn dashboard_ready(window: tauri::WebviewWindow) {
    READY.store(true, Ordering::SeqCst);
    let pending = PENDING_PAGE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(page) = pending {
        let _ = window.emit_to(LABEL, "arcade://dashboard-page", page);
    }
}

#[tauri::command]
pub(crate) fn open_dashboard(app: tauri::AppHandle, page: Option<String>) {
    open(&app, page.as_deref());
}

/// A tool card or a job result in the dashboard opens that tool in the Island.
#[tauri::command]
pub(crate) fn open_tool_in_island(app: tauri::AppHandle, tool: String, job: Option<String>) {
    crate::open_island_tool(&app, serde_json::json!({ "tool": tool, "job": job }));
}
