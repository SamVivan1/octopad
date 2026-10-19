mod commands;

use std::sync::Arc;

use octopad_core::bridge;
use octopad_core::config::{auto_import_legacy, ConfigStore};
use octopad_core::device::manager::DeviceManager;
use octopad_core::state::{AppEvent, AppState, Subscription};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use tokio::sync::{mpsc, watch};

struct RuntimeControl {
    shutdown: watch::Sender<bool>,
}

#[tauri::command]
fn get_version() -> String {
    "0.1.0".to_string()
}

#[tauri::command]
fn get_autostart_enabled_cmd(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_autostart_enabled_cmd(enabled: bool, app: AppHandle) -> Result<(), String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable()
    } else {
        manager.disable()
    }
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn quit_app_cmd(app: AppHandle) {
    app.exit(0);
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = window.show() {
            eprintln!("failed to show main window: {error}");
        }
        if let Err(error) = window.set_focus() {
            eprintln!("failed to focus main window: {error}");
        }
    }
}

fn tray_connection_status(state: &AppState) -> String {
    if state.serial_connected() {
        return format!(
            "Serial connected ({})",
            state.serial_port().as_deref().unwrap_or("unknown")
        );
    }
    if state.ble_connected() {
        let message = state.ble_message();
        return if message.is_empty() {
            "Bluetooth connected".to_string()
        } else {
            format!("Bluetooth: {message}")
        };
    }
    "Disconnected".to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if !args.iter().any(|argument| argument == "--hidden") {
                show_main_window(app);
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("OctoPad")
                .args(["--hidden"])
                .build(),
        )
        .setup(|app| {
            let start_hidden = std::env::args().any(|argument| argument == "--hidden");
            let window = app
                .get_webview_window("main")
                .ok_or("main application window is missing")?;
            if !start_hidden {
                window.show()?;
            }
            let close_window = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    if let Err(error) = close_window.hide() {
                        eprintln!("failed to hide main window: {error}");
                    }
                }
            });

            let store = ConfigStore::new_app();
            if let Err(error) = auto_import_legacy(&store) {
                eprintln!("automatic legacy config import failed: {error}");
            }
            let state = Arc::new(AppState::new(store));
            let manager = DeviceManager::new();
            let (device_tx, device_rx) = mpsc::channel(256);
            let (shutdown, shutdown_rx) = watch::channel(false);

            let open_item = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
            let status_item = MenuItem::with_id(
                app,
                "connection-status",
                tray_connection_status(&state),
                false,
                None::<&str>,
            )?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_item, &status_item, &quit_item])?;
            let icon = app
                .default_window_icon()
                .cloned()
                .ok_or("application tray icon is missing")?;
            TrayIconBuilder::new()
                .menu(&menu)
                .icon(icon)
                .tooltip("OctoPad")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            let app_handle = app.handle().clone();
            let tray_state = state.clone();
            let tray_status = status_item.clone();
            let subscription: Subscription = state.subscribe(move |event| {
                let (name, payload) = event.payload();
                let topic = format!("octopad://{name}");
                if let Err(error) = app_handle.emit(&topic, payload) {
                    eprintln!("failed to emit app event {topic}: {error}");
                }
                if matches!(
                    &event,
                    AppEvent::SerialStatusChanged { .. } | AppEvent::BleStatusChanged { .. }
                ) {
                    if let Err(error) = tray_status.set_text(tray_connection_status(&tray_state)) {
                        eprintln!("failed to update tray connection status: {error}");
                    }
                }
            });

            if !app.manage(state.clone())
                || !app.manage(manager.clone())
                || !app.manage(RuntimeControl {
                    shutdown: shutdown.clone(),
                })
                || !app.manage(subscription)
            {
                return Err("failed to initialize app runtime state".into());
            }

            manager.spawn(device_tx, shutdown_rx);
            bridge::connect_device_events(state, device_rx);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_version,
            commands::spikes::spike_legacy_config_dir,
            commands::spikes::spike_list_ports,
            commands::spikes::spike_probe_hello,
            commands::device::get_state_snapshot,
            commands::device::import_legacy_config,
            commands::device::save_config_cmd,
            commands::device::send_device_command_cmd,
            commands::device::set_mode_cmd,
            commands::device::select_button_cmd,
            commands::device::set_label_cmd,
            commands::device::set_action_cmd,
            commands::device::add_action_cmd,
            commands::device::remove_action_cmd,
            commands::device::move_action_cmd,
            commands::device::set_timing_cmd,
            commands::device::list_presets_cmd,
            commands::device::rename_device_cmd,
            get_autostart_enabled_cmd,
            set_autostart_enabled_cmd,
            quit_app_cmd
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
