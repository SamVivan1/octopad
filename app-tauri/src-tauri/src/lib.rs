//! OctoPad desktop app: window, tray residency, device runtime and commands.
//!
//! M2 residency rules, and where they live:
//!
//! - **Closing the window hides it.** `WindowEvent::CloseRequested` is
//!   prevented and the window is hidden, so serial/BLE connections keep running
//!   with no window on screen. Only [`shutdown_and_exit`] really ends the app.
//! - **The tray is the resident surface.** Its menu carries Open, a live
//!   connection-status line driven by the same events the frontend listens to,
//!   and Quit.
//! - **A second launch never starts a second runtime.** The single-instance
//!   plugin is registered first in the builder chain and asks the resident
//!   instance to show and focus its window.
//! - **`--hidden` starts in the tray.** That is the argument the autostart login
//!   entry carries, so an autostart launch never flashes a window.

mod commands;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use octopad_core::bridge;
use octopad_core::config::{auto_import_legacy, ConfigStore};
use octopad_core::device::manager::DeviceManager;
use octopad_core::state::{AppState, Subscription};
use serde_json::Value;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::{mpsc, watch};

/// Tray icon id, and the ids of its menu items.
const TRAY_ID: &str = "octopad-tray";
const MENU_OPEN: &str = "open";
const MENU_STATUS: &str = "status";
const MENU_QUIT: &str = "quit";

/// The main window's label; the tray and the single-instance handler address it
/// by name so there is exactly one place that knows it.
const MAIN_WINDOW: &str = "main";

/// Argument the OS autostart entry passes so a login launch stays in the tray.
const HIDDEN_ARG: &str = "--hidden";

/// Shutdown control shared by the app, the tray Quit item and `Ctrl+Q`.
struct RuntimeControl {
    shutdown: watch::Sender<bool>,
    /// Set before an intentional exit so the close-to-tray path stands down:
    /// an exiting app must not be told to hide a window it is about to lose.
    quitting: AtomicBool,
}

/// Connection state as the tray reports it, mirrored from the same events the
/// frontend receives, so the resident app still shows its state with no window.
#[derive(Clone, Copy, Default)]
struct ConnectionStatus {
    serial: bool,
    ble: bool,
}

impl ConnectionStatus {
    /// Tray menu text. Short enough for a system menu, and never optimistic.
    fn label(self) -> String {
        let text = match (self.serial, self.ble) {
            (true, true) => "Serial + Bluetooth",
            (true, false) => "Serial",
            (false, true) => "Bluetooth",
            (false, false) => "Not connected",
        };
        format!("Status: {text}")
    }
}

/// `connected` out of a device-status payload, if it carries one.
fn connection_flag(payload: &Value) -> Option<bool> {
    payload.get("connected").and_then(Value::as_bool)
}

/// Bring the main window back, whatever state the tray left it in.
fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// End the app for real: stop the device runtime, then exit.
///
/// The single exit path — the tray Quit item and `quit_app_cmd` (`Ctrl+Q`) both
/// call it, so quitting can never be reached by one and not the other.
fn shutdown_and_exit<R: Runtime>(app: &AppHandle<R>) {
    if let Some(runtime) = app.try_state::<RuntimeControl>() {
        runtime.quitting.store(true, Ordering::SeqCst);
        let _ = runtime.shutdown.send(true);
    }
    app.exit(0);
}

/// True once an intentional quit has begun.
fn is_quitting<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<RuntimeControl>()
        .map(|runtime| runtime.quitting.load(Ordering::SeqCst))
        .unwrap_or(false)
}

/// Build the tray: Open, a live status line, and Quit.
fn build_tray(
    app: &tauri::App,
) -> tauri::Result<(MenuItem<tauri::Wry>, Arc<Mutex<ConnectionStatus>>)> {
    let open = MenuItem::with_id(app, MENU_OPEN, "Open", true, None::<&str>)?;
    // Not clickable: it is a report of what the core last told us.
    let status = MenuItem::with_id(
        app,
        MENU_STATUS,
        ConnectionStatus::default().label(),
        false,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &status, &quit])?;

    let connection = Arc::new(Mutex::new(ConnectionStatus::default()));

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("OctoPad")
        .menu(&menu)
        // Linux/GTK convention here: the left button opens the app and the menu
        // opens on right click (on Windows the left click shows the menu).
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            MENU_OPEN => show_main_window(app),
            MENU_QUIT => shutdown_and_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    // The bundled window icon, so the tray matches the app and the installer.
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

    Ok((status, connection))
}

/// Mirror the core's device-status events into the tray status line.
fn update_tray_status(
    status: &MenuItem<tauri::Wry>,
    connection: &Mutex<ConnectionStatus>,
    name: &str,
    payload: &Value,
) {
    let Some(connected) = connection_flag(payload) else {
        return;
    };
    let label = match connection.lock() {
        Ok(mut guard) => {
            match name {
                "serial-status-changed" => guard.serial = connected,
                "ble-status-changed" => guard.ble = connected,
                _ => {}
            }
            guard.label()
        }
        Err(_) => return,
    };
    let _ = status.set_text(label);
}

/// Close-to-tray: a closed window is a hidden window.
fn handle_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    if window.label() != MAIN_WINDOW {
        return;
    }
    let tauri::WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    if is_quitting(window.app_handle()) {
        return;
    }
    // The runtime keeps running: hiding is what makes this app resident.
    api.prevent_close();
    let _ = window.hide();
}

#[tauri::command]
fn get_version() -> String {
    "0.1.0".to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // Registered first, before every other plugin: a second launch must be
        // intercepted before anything else initialises a second runtime.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // The resident instance already owns the device connections, so the
            // second launch only asks for the window back.
            show_main_window(app);
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("OctoPad")
                // The login entry starts the app in the tray; a manual launch has
                // no argument and shows the window as usual.
                .arg(HIDDEN_ARG)
                .build(),
        )
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let store = ConfigStore::new_app();
            if let Err(error) = auto_import_legacy(&store) {
                eprintln!("automatic legacy config import failed: {error}");
            }
            let state = Arc::new(AppState::new(store));
            let manager = DeviceManager::new();
            let (device_tx, device_rx) = mpsc::channel(256);
            let (shutdown, shutdown_rx) = watch::channel(false);

            let (tray_status, connection) = build_tray(app)?;

            let app_handle = app.handle().clone();
            let subscription: Subscription = state.subscribe(move |event| {
                let (name, payload) = event.payload();
                let topic = format!("octopad://{name}");
                if let Err(error) = app_handle.emit(&topic, &payload) {
                    eprintln!("failed to emit app event {topic}: {error}");
                }
                update_tray_status(&tray_status, &connection, name, &payload);
            });

            if !app.manage(state.clone())
                || !app.manage(manager.clone())
                || !app.manage(RuntimeControl {
                    shutdown: shutdown.clone(),
                    quitting: AtomicBool::new(false),
                })
                || !app.manage(subscription)
            {
                return Err("failed to initialize app runtime state".into());
            }

            // Both of these start tokio work — `manager.spawn` a blocking task,
            // `connect_device_events` a future — and the setup hook runs on the
            // main thread with no reactor entered, where tokio panics with
            // "there is no reactor running". Entering Tauri's async runtime for
            // exactly these two calls is what starts the device runtime; the
            // work inside is a spawn each, so this returns immediately.
            tauri::async_runtime::block_on(async {
                manager.spawn(device_tx, shutdown_rx);
                bridge::connect_device_events(state.clone(), device_rx);
            });

            // Autostart (`--hidden`): the window exists so the runtime and the
            // tray do, but nothing is shown until Open is used.
            if std::env::args().any(|argument| argument == HIDDEN_ARG) {
                if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                    let _ = window.hide();
                }
            }
            Ok(())
        })
        .on_window_event(handle_window_event)
        .invoke_handler(tauri::generate_handler![
            get_version,
            commands::quit_app_cmd,
            commands::spikes::spike_legacy_config_dir,
            commands::spikes::spike_list_ports,
            commands::spikes::spike_probe_hello,
            commands::device::get_state_snapshot,
            commands::device::import_legacy_config,
            commands::device::save_config_cmd,
            commands::device::list_presets_cmd,
            commands::device::rename_device_cmd,
            commands::device::set_mode_cmd,
            commands::device::select_button_cmd,
            commands::device::set_label_cmd,
            commands::device::set_action_cmd,
            commands::device::add_action_cmd,
            commands::device::remove_action_cmd,
            commands::device::move_action_cmd,
            commands::device::set_timing_cmd,
            commands::device::send_device_command_cmd
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            if let Some(runtime) = app_handle.try_state::<RuntimeControl>() {
                let _ = runtime.shutdown.send(true);
            }
        }
    });
}
