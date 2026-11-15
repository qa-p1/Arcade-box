//! Memory items from `AUDIT/memory.md` (D14): the resident gives back what a closed or hidden window left behind,
//! and trims itself while a window is open. Code refaults from the page cache in microseconds, so nothing here
//! changes how Box feels. No timer runs while Box has no window.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager};

/// OPEN1: the first trim after a page has loaded, then one every 20 s while any window is open.
const OPEN_TRIM_FIRST: Duration = Duration::from_secs(2);
const OPEN_TRIM_EVERY: Duration = Duration::from_secs(20);
/// W1: GTK and WebKit finish destroying a window first; the network process flushes its storage.
pub(crate) const RELEASE_AFTER_CLOSE: Duration = Duration::from_secs(3);
/// W0: startup has settled (Link, the tray and the shortcut are up).
pub(crate) const RELEASE_AFTER_STARTUP: Duration = Duration::from_secs(15);

static TRIMMING: AtomicBool = AtomicBool::new(false);

/// OPEN1: started when a page finishes loading; one thread for all windows, ending with the last one.
pub(crate) fn trim_while_open(app: &AppHandle) {
    if TRIMMING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(OPEN_TRIM_FIRST);
        loop {
            if app.webview_windows().is_empty() {
                TRIMMING.store(false, Ordering::SeqCst);
                // A page that loaded just now saw the flag still set and left the trimming to this thread.
                if app.webview_windows().is_empty() || TRIMMING.swap(true, Ordering::SeqCst) {
                    return;
                }
            } else {
                arcade_link::runtime::trim();
            }
            std::thread::sleep(OPEN_TRIM_EVERY);
        }
    });
}

/// W1 + W3b, and W0 at startup: once no window is left, end WebKit's network process (WebKit starts a new one
/// with the next window), return freed heap and code pages, and let the kernel page out the idle heap (free
/// unless memory is short). With a window still open it only trims.
pub(crate) fn reclaim_after(app: &AppHandle, delay: Duration) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        if app.webview_windows().is_empty() {
            arcade_link::runtime::release_webkit();
            arcade_link::runtime::page_out_hidden();
        } else {
            arcade_link::runtime::trim();
        }
    });
}

/// W0 after background work (a Link request served while hidden): give back what it faulted in.
pub(crate) fn trim_if_hidden(app: &AppHandle) {
    if app.webview_windows().is_empty() {
        arcade_link::runtime::trim();
    }
}

/// OPEN1: WebKit's helper processes inherit `libarcade-trim.so` (vendored from Link, built by
/// `scripts/prepare-bundle.py`), which trims them 2 s after start and every 20 s; it does nothing in any other
/// process. Programs Box starts never get it: Link's child environment drops entries inside `$APPDIR`. Call it
/// while Box still has one thread.
#[cfg(target_os = "linux")]
pub fn preload_webkit_trimmer() {
    let Some(dir) = std::env::var_os("APPDIR").filter(|d| !d.is_empty()) else {
        return;
    };
    let library = std::path::Path::new(&dir).join("usr/lib/libarcade-trim.so");
    if !library.is_file() {
        return;
    }
    let mut value = library.into_os_string();
    if let Some(old) = std::env::var_os("LD_PRELOAD").filter(|v| !v.is_empty()) {
        value.push(":");
        value.push(old);
    }
    // SAFETY: called from `main` before Tauri, WebKit or any other thread starts.
    unsafe { std::env::set_var("LD_PRELOAD", value) };
}
