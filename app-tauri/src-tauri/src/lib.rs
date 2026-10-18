mod commands;

use std::sync::Arc;

use octopad_core::bridge;
use octopad_core::config::{auto_import_legacy, ConfigStore};
use octopad_core::device::manager::DeviceManager;
use octopad_core::state::{AppState, Subscription};
use tauri::{Emitter, Manager};
use tokio::sync::{mpsc, watch};

struct RuntimeControl {
    shutdown: watch::Sender<bool>,
}

#[tauri::command]
fn get_version() -> String {
    "0.1.0".to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
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

            let app_handle = app.handle().clone();
            let subscription: Subscription = state.subscribe(move |event| {
                let (name, payload) = event.payload();
                let topic = format!("octopad://{name}");
                if let Err(error) = app_handle.emit(&topic, payload) {
                    eprintln!("failed to emit app event {topic}: {error}");
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
            commands::device::rename_device_cmd
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
