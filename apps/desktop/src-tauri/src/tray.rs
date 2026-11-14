//! Box's tray icon: the native one (ksni on Linux, Tauri's elsewhere), the recovery tray while Tools hosts the
//! icon, and the watcher that follows Tools' tray host.

use super::*;

#[cfg(not(target_os = "linux"))]
pub(crate) struct PreparedRecoveryTray {
    menu: Menu<tauri::Wry>,
    icon: tauri::image::Image<'static>,
}

#[cfg(not(target_os = "linux"))]
pub(crate) struct RecoveryTray {
    prepared: Option<PreparedRecoveryTray>,
    icon: Option<tauri::tray::TrayIcon>,
}

#[cfg(not(target_os = "linux"))]
thread_local! {
    static RECOVERY_TRAY: std::cell::RefCell<Option<RecoveryTray>> = const { std::cell::RefCell::new(None) };
}

/// The tray icon: Box's glyph, monochrome (STANDARD §8). White on Linux
/// panels (dark on GNOME, Plasma's default and the usual Hyprland bars),
/// black on macOS (a template image the menu bar recolours), and the
/// taskbar's contrast colour on Windows.
pub(crate) fn tray_glyph_png() -> &'static [u8] {
    if tray_panel_is_dark() {
        include_bytes!("../icons/tray-white.png")
    } else {
        include_bytes!("../icons/tray-black.png")
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn tray_panel_is_dark() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
        .encode_utf16()
        .collect();
    let name: Vec<u16> = "SystemUsesLightTheme\0".encode_utf16().collect();
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: both strings are NUL-terminated and the output is a u32.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    // A missing value means the classic dark taskbar.
    status != 0 || value == 0
}

#[cfg(target_os = "macos")]
pub(crate) fn tray_panel_is_dark() -> bool {
    false
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn tray_panel_is_dark() -> bool {
    true
}

/// Construct toolkit resources before the first host answer, but delay OS
/// tray registration until Box owns its icon (tray-icon has no hidden ctor).
#[cfg(not(target_os = "linux"))]
pub(crate) fn prepare_recovery_tray<M: tauri::Manager<tauri::Wry>>(
    app: &M,
) -> tauri::Result<PreparedRecoveryTray> {
    let toggle = MenuItem::with_id(
        app,
        "toggle-island",
        "Show or hide Arcade Box",
        true,
        None::<&str>,
    )?;
    let dashboard = MenuItem::with_id(app, "open-dashboard", "Open dashboard", true, None::<&str>)?;
    let pipelines = MenuItem::with_id(app, "open-pipelines", "Open pipelines", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let restart_item = MenuItem::with_id(app, "restart", "Restart Arcade Box", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Arcade Box", true, None::<&str>)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &toggle,
            &dashboard,
            &pipelines,
            &settings,
            &separator,
            &restart_item,
            &quit,
        ],
    )?;
    let icon = tauri::image::Image::from_bytes(tray_glyph_png())?;
    Ok(PreparedRecoveryTray { menu, icon })
}

pub(crate) fn set_recovery_tray(app: &tauri::AppHandle, state: arcade_link::trayhost::TrayState) {
    #[cfg(target_os = "linux")]
    linux_tray::set_state(app, state);
    #[cfg(not(target_os = "linux"))]
    set_native_tray(app, state);
}

/// Linux draws the tray over D-Bus with ksni. Only Own registers an item:
/// hosts such as Caelestia and Waybar still show Passive items, and Tauri's
/// libappindicator item stays registered (Passive) after removal, so a hosted
/// icon is unregistered by shutting the ksni service down.
#[cfg(target_os = "linux")]
pub(crate) mod linux_tray {
    use arcade_link::trayhost::TrayState;
    use ksni::TrayMethods;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::AppHandle;

    struct Tray {
        app: AppHandle,
    }

    fn on_main(app: &AppHandle, action: fn(&AppHandle)) {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || action(&handle));
    }
    fn toggle(app: &AppHandle) {
        super::toggle_island_app(app);
    }
    fn quit(app: &AppHandle) {
        app.exit(0);
    }

    impl ksni::Tray for Tray {
        fn id(&self) -> String {
            "arcade-box".into()
        }
        fn title(&self) -> String {
            "Arcade Box".into()
        }
        fn category(&self) -> ksni::Category {
            ksni::Category::ApplicationStatus
        }
        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            static ICONS: OnceLock<Vec<ksni::Icon>> = OnceLock::new();
            ICONS
                .get_or_init(|| {
                    image::load_from_memory(super::tray_glyph_png())
                        .map(|image| {
                            [32, 64, 128]
                                .into_iter()
                                .map(|size| {
                                    let image = image
                                        .resize_exact(
                                            size,
                                            size,
                                            image::imageops::FilterType::Lanczos3,
                                        )
                                        .into_rgba8();
                                    let mut data = image.into_vec();
                                    // RGBA → ARGB32 in network byte order.
                                    for pixel in data.chunks_exact_mut(4) {
                                        pixel.rotate_right(1);
                                    }
                                    ksni::Icon {
                                        width: size as i32,
                                        height: size as i32,
                                        data,
                                    }
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .clone()
        }
        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "Arcade Box".into(),
                ..Default::default()
            }
        }
        fn activate(&mut self, _x: i32, _y: i32) {
            on_main(&self.app, super::open_dashboard);
        }
        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            use ksni::menu::StandardItem;
            let item = |label: &str, action: fn(&AppHandle)| -> ksni::MenuItem<Self> {
                StandardItem {
                    label: label.into(),
                    activate: Box::new(move |tray: &mut Self| on_main(&tray.app, action)),
                    ..Default::default()
                }
                .into()
            };
            vec![
                item("Show or hide Arcade Box", toggle),
                item("Open dashboard", super::open_dashboard),
                item("Open pipelines", super::open_pipelines),
                item("Settings", super::open_settings),
                ksni::MenuItem::Separator,
                item("Restart Arcade Box", super::restart),
                item("Quit Arcade Box", quit),
            ]
        }
    }

    /// The latest requested state; one serialized task reconciles to it, so an
    /// old item is gone before a new one registers.
    static VISIBLE: AtomicBool = AtomicBool::new(false);
    static HANDLE: OnceLock<tauri::async_runtime::Mutex<Option<ksni::Handle<Tray>>>> =
        OnceLock::new();

    pub fn set_state(app: &AppHandle, state: TrayState) {
        VISIBLE.store(state == TrayState::Own, Ordering::SeqCst);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut handle = HANDLE
                .get_or_init(|| tauri::async_runtime::Mutex::new(None))
                .lock()
                .await;
            if handle.as_ref().is_some_and(ksni::Handle::is_closed) {
                *handle = None;
            }
            let visible = VISIBLE.load(Ordering::SeqCst);
            if visible && handle.is_none() {
                match (Tray { app }).assume_sni_available(true).spawn().await {
                    Ok(spawned) => *handle = Some(spawned),
                    Err(error) => eprintln!("Arcade Box tray registration failed: {error}"),
                }
            } else if !visible {
                if let Some(old) = handle.take() {
                    old.shutdown().await;
                }
            }
        });
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn set_native_tray(app: &tauri::AppHandle, state: arcade_link::trayhost::TrayState) {
    RECOVERY_TRAY.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let prepared = prepare_recovery_tray(app).map_err(|error| error.to_string());
            match prepared {
                Ok(prepared) => {
                    *slot = Some(RecoveryTray {
                        prepared: Some(prepared),
                        icon: None,
                    })
                }
                Err(error) => {
                    eprintln!("Arcade Box tray preparation failed: {error}");
                    return;
                }
            }
        }
        let Some(tray) = slot.as_mut() else { return };
        match state {
            arcade_link::trayhost::TrayState::Hosted | arcade_link::trayhost::TrayState::None => {
                if let Some(icon) = &tray.icon {
                    if let Err(error) = icon.set_visible(false) {
                        eprintln!("Arcade Box tray hide failed: {error}");
                    }
                }
            }
            arcade_link::trayhost::TrayState::Own => {
                if tray.icon.is_none() {
                    let prepared = match tray.prepared.take() {
                        Some(prepared) => prepared,
                        None => match prepare_recovery_tray(app) {
                            Ok(prepared) => prepared,
                            Err(error) => {
                                eprintln!("Arcade Box tray preparation failed: {error}");
                                return;
                            }
                        },
                    };
                    let menu_on_left = cfg!(target_os = "macos");
                    let built = TrayIconBuilder::with_id("arcade-box-tray")
                        .tooltip("Arcade Box")
                        .menu(&prepared.menu)
                        .icon(prepared.icon)
                        .icon_as_template(cfg!(target_os = "macos"))
                        .show_menu_on_left_click(menu_on_left)
                        .on_menu_event(|app, event| match event.id().as_ref() {
                            "toggle-island" => toggle_island_app(app),
                            "open-dashboard" => open_dashboard(app),
                            "open-pipelines" => open_pipelines(app),
                            "settings" => open_settings(app),
                            "restart" => restart(app),
                            "quit" => app.exit(0),
                            _ => {}
                        })
                        .on_tray_icon_event(move |tray, event| {
                            if let tauri::tray::TrayIconEvent::Click {
                                button: tauri::tray::MouseButton::Left,
                                button_state: tauri::tray::MouseButtonState::Up,
                                ..
                            } = event
                                && !menu_on_left
                            {
                                open_dashboard(tray.app_handle());
                            }
                        })
                        .build(app);
                    match built {
                        Ok(icon) => tray.icon = Some(icon),
                        Err(error) => {
                            eprintln!("Arcade Box tray registration failed: {error}");
                            tray.prepared = prepare_recovery_tray(app).ok();
                            return;
                        }
                    }
                }
                if let Some(icon) = &tray.icon {
                    if let Err(error) = icon.set_visible(true) {
                        eprintln!("Arcade Box tray show failed: {error}");
                    }
                }
            }
        }
    });
}

pub(crate) fn start_tray_host_watcher(
    app: &tauri::AppHandle,
    runtime: &Arc<Arcade>,
) -> Result<(), String> {
    use arcade_link::trayhost::{TrayHostWatcher, WatcherConfig};
    let app_for_updates = app.clone();
    let watcher = TrayHostWatcher::start(
        WatcherConfig {
            locations: arcade_link::Locations::discover(),
            app: arcade_link::PeerInfo {
                id: arcade_link::ids::BOX.into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            link_enabled: arcade_core::link::LinkSettings::load(runtime).enabled,
            has_tray: true,
        },
        move |state| {
            link::set_tray_state(state);
            let app = app_for_updates.clone();
            let _ = app_for_updates.run_on_main_thread(move || set_recovery_tray(&app, state));
        },
    )
    .map_err(|error| error.to_string())?;
    app.manage(watcher);
    Ok(())
}

/// Build the tray state before watching. A hosted startup retains the menu and
/// image without registering a duplicate OS icon.
#[cfg(not(target_os = "linux"))]
pub(crate) fn initialize_recovery_tray(app: &tauri::App) -> tauri::Result<()> {
    let prepared = prepare_recovery_tray(app)?;
    RECOVERY_TRAY.with(|slot| {
        *slot.borrow_mut() = Some(RecoveryTray {
            prepared: Some(prepared),
            icon: None,
        });
    });
    Ok(())
}
