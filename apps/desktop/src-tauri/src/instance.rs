//! The resident process: restart with a successor, waiting for a predecessor, the second-instance handoff and
//! `--status`.

use super::*;

/// Set on the successor so it waits for this process to exit before taking
/// the single-instance slot and the profile lock.
pub(crate) const RESTART_ENV: &str = "ARCADE_BOX_RESTART_AFTER";

/// Starts a new background instance, then quits. Quitting without a
/// successor would silently remove Box, so a failed launch keeps this one.
pub(crate) fn restart(app: &tauri::AppHandle) {
    let mut command = std::process::Command::new(arcade_link::manifest::current_executable());
    // The successor runs from a new mount; it mustn't keep this one alive.
    let started = arcade_link::childenv::sanitize(&mut command)
        .arg("--background")
        .env(RESTART_ENV, std::process::id().to_string())
        .spawn();
    match started {
        Ok(_) => app.exit(0),
        Err(error) => {
            eprintln!("Arcade Box could not start a new instance, so it kept running: {error}")
        }
    }
}

/// In a successor started by [`restart`]: waits (up to 5 s) for the old
/// instance to exit.
pub(crate) fn wait_for_predecessor() {
    let Some(pid) = std::env::var(RESTART_ENV)
        .ok()
        .and_then(|p| p.parse::<u32>().ok())
    else {
        return;
    };
    for _ in 0..50 {
        if !process_alive(pid) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(target_os = "macos")]
pub(crate) fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(windows)]
pub(crate) fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    // SAFETY: plain Win32 calls on a handle we own and close.
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let alive = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        CloseHandle(handle);
        alive
    }
}

/// On Windows the release build has no console; attach to the parent's so
/// `--version` and `--arcade-manifest` print where they were asked.
pub fn attach_console() {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
}

/// Another launch of Arcade Box (the applications menu, `--settings`,
/// `--quit`) talks to this instance instead of starting a second one.
pub(crate) fn handle_second_instance(app: &tauri::AppHandle, argv: &[String]) {
    let flag = argv
        .iter()
        .skip(1)
        .find(|a| a.starts_with("--"))
        .map(String::as_str);
    match flag {
        Some("--quit") => app.exit(0),
        Some("--settings") => open_settings(app),
        Some("--restart") => restart(app),
        Some("--pipelines") => open_pipelines(app),
        Some("--dashboard") => open_dashboard(app),
        Some("--toggle") => toggle_island_app(app),
        Some("--hide") => hide_island_app(app),
        Some("--background") => {}
        // `--show` and a plain launch.
        _ => open_island(app),
    }
}

/// `--status`: one line of JSON (running, version, Link state) without
/// starting anything.
pub fn print_status() {
    let locations = arcade_link::Locations::discover();
    let registry = arcade_link::Registry::load(&locations);
    let me = arcade_link::PeerInfo {
        id: format!("{}.status", installer::ME),
        version: env!("CARGO_PKG_VERSION").into(),
    };
    let running = match arcade_link::client::app_state(&locations, &registry, installer::ME, &me) {
        arcade_link::AppState::Running { version } => Some(version),
        _ => None,
    };
    let link = registry
        .get(installer::ME)
        .map_or("unregistered", |manifest| {
            if manifest.settings.link_enabled {
                "on"
            } else {
                "off"
            }
        });
    println!(
        "{}",
        serde_json::json!({
            "app": installer::ME,
            "version": env!("CARGO_PKG_VERSION"),
            "running": running.is_some(),
            "runningVersion": running,
            "link": link,
        })
    );
}
