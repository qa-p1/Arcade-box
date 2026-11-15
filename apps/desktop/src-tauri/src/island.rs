//! The Island window: showing, hiding and releasing it, fitting it to its card and placing it on the active
//! monitor, and the Link and tool opens that wait for it to be ready.

use super::*;

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct SurfaceGeometry {
    width: f64,
    height: f64,
}

#[derive(Default)]
pub(crate) struct IslandVisibility {
    generation: u64,
    hiding: bool,
    ready: bool,
    pending_show: bool,
}

pub(crate) static ISLAND_VISIBILITY: Mutex<IslandVisibility> = Mutex::new(IslandVisibility {
    generation: 0,
    hiding: false,
    ready: false,
    pending_show: false,
});

pub(crate) static SURFACE_GEOMETRY: Mutex<SurfaceGeometry> = Mutex::new(SurfaceGeometry {
    width: 740.0,
    height: 740.0,
});

#[tauri::command]
pub(crate) fn hide_island(window: tauri::WebviewWindow) -> Result<(), String> {
    request_hide_island(&window, ISLAND_IDLE_RELEASE);
    Ok(())
}

// Async so window reconfiguration and Hyprland queries never block the WebView
// thread while the Island is animating between states.
#[tauri::command]
pub(crate) async fn set_surface_mode(
    mode: String,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    apply_surface_mode(&mode, &window, false)
}

pub(crate) fn apply_surface_mode(
    mode: &str,
    window: &tauri::WebviewWindow,
    force: bool,
) -> Result<(), String> {
    // The dashboard is its own window; the Island's states share one canvas.
    if !matches!(mode, "compact" | "search" | "tool") {
        return Err("Unknown surface mode".into());
    }
    let (width, height) = fit_surface_size(window, 740.0, 740.0);
    let next = SurfaceGeometry { width, height };
    {
        let mut geometry = SURFACE_GEOMETRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Compact, search, and tool states share one canvas. Only the web
        // surface changes between them, so skip native work entirely.
        if !force && *geometry == next {
            return Ok(());
        }
        *geometry = next;
    }
    window
        .set_min_size(Some(Size::Logical(LogicalSize::new(
            width.min(320.0),
            height.min(116.0),
        ))))
        .map_err(|error| error.to_string())?;
    window
        .set_size(Size::Logical(LogicalSize::new(width, height)))
        .map_err(|error| error.to_string())?;
    let _ = window.set_always_on_top(true);
    let _ = window.set_skip_taskbar(true);
    place_surface(window);
    #[cfg(target_os = "linux")]
    if window.is_visible().unwrap_or(false) {
        let _ = hyprland::place_island(width, height, false);
    }
    Ok(())
}

pub(crate) fn active_monitor(window: &tauri::WebviewWindow) -> Option<tauri::Monitor> {
    window
        .cursor_position()
        .ok()
        .and_then(|cursor| window.monitor_from_point(cursor.x, cursor.y).ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten())
}

pub(crate) fn fit_surface_size(
    window: &tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> (f64, f64) {
    let Some(monitor) = active_monitor(window) else {
        return (width, height);
    };
    let scale = monitor.scale_factor().max(1.0);
    let screen = monitor.size();
    let max_width = ((screen.width as f64 / scale) - 48.0).max(1.0);
    let max_height = ((screen.height as f64 / scale) - 48.0).max(1.0);
    (width.min(max_width), height.min(max_height))
}

/// Centres the Island horizontally at the top of the active monitor.
pub(crate) fn place_surface(window: &tauri::WebviewWindow) {
    let Some(monitor) = active_monitor(window) else {
        return;
    };
    let screen = monitor.size();
    let origin = monitor.position();
    let Ok(size) = window.outer_size() else {
        return;
    };
    let x_offset = (i64::from(screen.width) - i64::from(size.width)).max(0) / 2;
    let x = (i64::from(origin.x) + x_offset).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let y = origin.y;
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(x, y)));
}

#[tauri::command]
pub(crate) fn island_ready(window: tauri::WebviewWindow) {
    let pending = {
        let mut state = ISLAND_VISIBILITY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.ready = true;
        std::mem::take(&mut state.pending_show)
    };
    eprintln!(
        "Arcade Island: page ready, a show is {}",
        if pending { "pending" } else { "not pending" }
    );
    if pending {
        show_island(&window);
    } else {
        // Created for a request that was cancelled before the page loaded.
        schedule_island_release(&window, ISLAND_IDLE_RELEASE);
    }
    let tool_opens = std::mem::take(
        &mut *PENDING_TOOL_OPENS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    for open in tool_opens {
        let _ = window.emit_to("island", "arcade://open-tool", open);
    }
    let link_opens = std::mem::take(
        &mut *PENDING_LINK_OPENS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    for open in link_opens {
        let _ = window.emit("arcade://link-open", open);
    }
}

/// Bottom edge (logical px) of the Island's visible card, from the last input region.
#[cfg(target_os = "linux")]
pub(crate) static ISLAND_REGION_BOTTOM: Mutex<Option<f64>> = Mutex::new(None);
#[cfg(target_os = "linux")]
pub(crate) static ISLAND_FIT: Mutex<(u64, f64)> = Mutex::new((0, 0.0));
/// Room below the card for its shadow.
#[cfg(target_os = "linux")]
pub(crate) const ISLAND_SHADOW_ROOM: f64 = 24.0;
/// Longer than the card's height animation, so a shrinking card is never clipped.
#[cfg(target_os = "linux")]
pub(crate) const ISLAND_SHRINK_DELAY: std::time::Duration = std::time::Duration::from_millis(360);

/// BR-2: Hyprland hit-tests a floating window by its box and ignores the surface's input region
/// (`ViewHitTester::windowAt`, 0.56), so the transparent canvas below the card took clicks meant for the window
/// underneath. There the window itself follows the card: it grows at once and shrinks once the animation ends.
/// Other desktops honour the input region and keep the full canvas.
#[cfg(target_os = "linux")]
pub(crate) fn fit_island_window(window: &tauri::WebviewWindow) {
    if !hyprland::is_hyprland() || !window.is_visible().unwrap_or(false) {
        return;
    }
    let Some(bottom) = *ISLAND_REGION_BOTTOM
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    else {
        return;
    };
    let canvas = *SURFACE_GEOMETRY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let target = (bottom + ISLAND_SHADOW_ROOM)
        .ceil()
        .clamp(1.0, canvas.height);
    let (generation, shrink) = {
        let mut fit = ISLAND_FIT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if (fit.1 - target).abs() < 1.0 {
            return;
        }
        fit.0 = fit.0.wrapping_add(1);
        let shrink = target < fit.1;
        if !shrink {
            fit.1 = target;
        }
        (fit.0, shrink)
    };
    std::thread::spawn(move || {
        if shrink {
            std::thread::sleep(ISLAND_SHRINK_DELAY);
            let mut fit = ISLAND_FIT
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if fit.0 != generation {
                return;
            }
            fit.1 = target;
        }
        hyprland::place_island(canvas.width, target, false);
    });
}

/// Restrict pointer input to the visible surface. The transparent canvas exists
/// for the spring/resize animation; it must never block the desktop beneath it.
#[tauri::command]
pub(crate) fn set_island_input_region(
    window: tauri::WebviewWindow,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if [x, y, width, height]
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 32768.0)
        || width < 0.0
        || height < 0.0
    {
        return Err("Invalid island input bounds".into());
    }
    #[cfg(target_os = "linux")]
    {
        let island = window.clone();
        window
            .run_on_main_thread(move || {
                use gtk::prelude::WidgetExt;
                if let Ok(native) = island.gtk_window() {
                    let region =
                        gtk::cairo::Region::create_rectangle(&gtk::cairo::RectangleInt::new(
                            x.floor() as i32,
                            y.floor() as i32,
                            width.ceil() as i32,
                            height.ceil() as i32,
                        ));
                    native.input_shape_combine_region(Some(&region));
                }
            })
            .map_err(|error| error.to_string())?;
        *ISLAND_REGION_BOTTOM
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(y + height);
        fit_island_window(&window);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = window;
    Ok(())
}

pub(crate) fn show_island(window: &tauri::WebviewWindow) {
    let island = window.clone();
    let _ = window.run_on_main_thread(move || {
        {
            let mut state = ISLAND_VISIBILITY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !state.ready {
                state.pending_show = true;
                eprintln!("Arcade Island: show waits for its page to be ready");
                return;
            }
            state.generation = state.generation.wrapping_add(1);
            state.hiding = false;
        }
        let geometry = *SURFACE_GEOMETRY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (width, height) = fit_surface_size(&island, geometry.width, geometry.height);
        let _ = island.set_size(Size::Logical(LogicalSize::new(width, height)));
        place_surface(&island);
        #[cfg(target_os = "linux")]
        let requested = stamp_user_time(&island);
        #[cfg(target_os = "linux")]
        crate::hyprland::remember_active_window();
        if let Err(error) = island.show() {
            eprintln!("Could not show Arcade Island: {error}");
            return;
        }
        #[cfg(target_os = "linux")]
        if let Some((native, time)) = requested {
            use gtk::prelude::GtkWindowExt;
            native.present_with_time(time);
        }
        let _ = island.set_focus();
        // The window was just set to the full canvas: fit it to the card again (BR-2).
        #[cfg(target_os = "linux")]
        {
            ISLAND_FIT
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .1 = height;
            fit_island_window(&island);
        }
        // TEMPORARY_MOTION_EVAL_START
        if std::env::var_os("ARCADE_MOTION_PROBE").is_some() {
            let _ = island.eval(include_str!("motion-probe.js"));
        }
        // TEMPORARY_MOTION_EVAL_END
        let _ = island.emit("arcade://island-shown", ());
    });
}

/// The Island opens from a global shortcut that GTK never sees, so its user
/// time stays stale, and X11 window managers that prevent focus stealing
/// (Openbox, for one) showed it without keyboard focus: what the user typed
/// went nowhere. Stamping it with the X server's current time before it maps,
/// and presenting it with that time, counts as the user's own request. Returns
/// the window and the time; `None` where the window isn't an X11 one (Wayland).
#[cfg(target_os = "linux")]
pub(crate) fn stamp_user_time(
    window: &tauri::WebviewWindow,
) -> Option<(gtk::ApplicationWindow, u32)> {
    use gtk::glib::Cast;
    use gtk::prelude::WidgetExt;
    let native = window.gtk_window().ok()?;
    native.realize();
    let x11 = native.window()?.downcast::<gdkx11::X11Window>().ok()?;
    let time = gdkx11::functions::x11_get_server_time(&x11);
    x11.set_user_time(time);
    Some((native, time))
}

pub(crate) fn toggle_island(window: &tauri::WebviewWindow) {
    let island = window.clone();
    let _ = window.run_on_main_thread(move || {
        let hiding = {
            let mut state = ISLAND_VISIBILITY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !state.ready && state.pending_show {
                // A second press while the WebView is still loading.
                state.pending_show = false;
                eprintln!("Arcade Island: toggled again before its page was ready, the pending show is cancelled");
                return;
            }
            state.hiding
        };
        if hiding || !island.is_visible().unwrap_or(false) {
            show_island(&island);
        } else {
            request_hide_island(&island, ISLAND_IDLE_RELEASE);
        }
    });
}

/// The Island's WebView is created on demand and destroyed once it has been
/// hidden this long, so a resident Box holds no WebKit/WebView2 processes
/// while idle (SPEC §17.5). The short grace keeps quick re-toggles instant.
pub(crate) const ISLAND_IDLE_RELEASE: std::time::Duration = std::time::Duration::from_secs(15);

/// `box.open` requests that arrived before the Island's WebView was ready.
pub(crate) static PENDING_LINK_OPENS: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());
/// Tools the dashboard opened before the Island's WebView was ready.
pub(crate) static PENDING_TOOL_OPENS: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

/// Runs `action` on the main thread with the Island, creating its WebView
/// first when none exists. Nothing in Box's background paths (tray, Link,
/// jobs) needs the WebView, so this is only called for UI requests.
pub(crate) fn with_island(
    app: &tauri::AppHandle,
    action: impl FnOnce(&tauri::WebviewWindow) + Send + 'static,
) {
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || match island_window(&handle) {
        Some(window) => action(&window),
        None => eprintln!("Arcade Island: no window, the request is dropped"),
    }) {
        eprintln!("Arcade Island: could not reach the main thread: {error}");
    }
}

/// Shows the Island, creating it if needed.
pub(crate) fn open_island(app: &tauri::AppHandle) {
    with_island(app, show_island);
}

/// Toggles the Island, creating it (shown) if needed.
pub(crate) fn toggle_island_app(app: &tauri::AppHandle) {
    with_island(app, toggle_island);
}

/// Delivers a `box.open` request to the Island, now or once it has loaded.
pub(crate) fn open_link_request(app: &tauri::AppHandle, open: serde_json::Value) {
    with_island(app, move |window| {
        let ready = ISLAND_VISIBILITY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ready;
        show_island(window);
        if ready {
            let _ = window.emit("arcade://link-open", open);
        } else {
            PENDING_LINK_OPENS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(open);
        }
    });
}

/// Opens a tool (and optionally a job's result) in the Island for the dashboard.
pub(crate) fn open_island_tool(app: &tauri::AppHandle, open: serde_json::Value) {
    with_island(app, move |window| {
        let ready = ISLAND_VISIBILITY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ready;
        show_island(window);
        if ready {
            let _ = window.emit_to("island", "arcade://open-tool", open);
        } else {
            PENDING_TOOL_OPENS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(open);
        }
    });
}

/// The existing Island, or a new one built from its `tauri.conf.json` entry
/// (`"create": false`). Must run on the main thread.
pub(crate) fn island_window(app: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window("island") {
        return Some(window);
    }
    let Some(config) = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "island")
        .cloned()
    else {
        eprintln!("Could not create Arcade Island: the configuration has no `island` window");
        return None;
    };
    eprintln!("Arcade Island: creating its WebView");
    {
        let mut state = ISLAND_VISIBILITY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.ready = false;
        state.hiding = false;
    }
    let window = match tauri::WebviewWindowBuilder::from_config(app, &config)
        .map(in_box_folder)
        .and_then(tauri::WebviewWindowBuilder::build)
    {
        Ok(window) => window,
        Err(error) => {
            eprintln!("Could not create Arcade Island: {error}");
            return None;
        }
    };
    #[cfg(target_os = "linux")]
    if let Err(error) = release_webview_ipc_on_destroy(&window) {
        eprintln!("Could not attach Island WebView cleanup: {error}");
    }
    let close_window = window.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            // Closing the window releases its WebView; Box stays resident.
            api.prevent_close();
            request_hide_island(&close_window, std::time::Duration::ZERO);
        }
        tauri::WindowEvent::Destroyed => {
            let mut state = ISLAND_VISIBILITY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.ready = false;
            state.hiding = false;
        }
        _ => {}
    });
    let _ = apply_surface_mode("compact", &window, true);
    eprintln!("Arcade Island: WebView created, it shows once its page is ready");
    Some(window)
}

/// Destroys the Island's WebView after `delay` unless it was shown again.
pub(crate) fn schedule_island_release(window: &tauri::WebviewWindow, delay: std::time::Duration) {
    let generation = ISLAND_VISIBILITY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .generation;
    let island = window.clone();
    tauri::async_runtime::spawn(async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        let release = island.clone();
        let _ = island.run_on_main_thread(move || {
            let state = ISLAND_VISIBILITY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let shown_again = state.generation != generation || state.hiding || state.pending_show;
            drop(state);
            if !shown_again && !release.is_visible().unwrap_or(false) {
                let _ = release.destroy();
            }
        });
    });
}

pub(crate) fn request_hide_island(
    window: &tauri::WebviewWindow,
    release_after: std::time::Duration,
) {
    let island = window.clone();
    let _ = window.run_on_main_thread(move || {
        let generation = {
            let mut state = ISLAND_VISIBILITY
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.hiding {
                return;
            }
            state.hiding = true;
            state.generation
        };
        let _ = island.emit("arcade://island-hiding", ());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(140)).await;
            let finish = island.clone();
            let _ = island.run_on_main_thread(move || {
                let mut state = ISLAND_VISIBILITY
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                // A second shortcut can reopen the island during its exit.
                if state.generation != generation || !state.hiding {
                    return;
                }
                if let Err(error) = finish.hide() {
                    eprintln!("Could not hide Arcade Island: {error}");
                    let _ = finish.emit("arcade://island-shown", ());
                    state.hiding = false;
                } else {
                    let _ = finish.emit("arcade://island-hidden", ());
                    state.hiding = false;
                    drop(state);
                    schedule_island_release(&finish, release_after);
                }
            });
        });
    });
}

/// `--hide`: hides the Island if it is open; nothing else changes.
pub(crate) fn hide_island_app(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("island") {
        request_hide_island(&window, std::time::Duration::ZERO);
    }
}
