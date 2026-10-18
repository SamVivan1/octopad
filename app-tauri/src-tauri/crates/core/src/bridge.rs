//! Bridge device events and import results into the headless application state.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::config::{import_legacy, ConfigStore, ImportError, ImportReport};
use crate::config_paths::legacy_config_dir;
use crate::device::manager::DeviceManager;
use crate::device::{DeviceEvent, TransportKind};
use crate::state::AppState;

/// Consume transport events and apply their state/event effects.
pub fn connect_device_events(
    state: Arc<AppState>,
    mut rx: mpsc::Receiver<DeviceEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                DeviceEvent::Log { line } => {
                    state.log(line);
                }
                DeviceEvent::StatusChanged {
                    transport: TransportKind::Serial,
                    connected,
                    port,
                    ..
                } => state.set_serial_status(connected, port),
                DeviceEvent::StatusChanged {
                    transport: TransportKind::Ble,
                    connected,
                    message,
                    ..
                } => state.set_ble_status(connected, message.as_deref().unwrap_or_default()),
                DeviceEvent::HelloReceived { name, version } => {
                    state.set_device_name(name);
                    state.set_firmware_version(Some(version));
                }
                DeviceEvent::ButtonEvent {
                    mode,
                    index,
                    trigger,
                } => {
                    let line = format!(
                        "[{}] Button {} -> {}",
                        mode.to_uppercase(),
                        index.saturating_add(1),
                        trigger.to_uppercase()
                    );
                    state.log(line);
                }
                DeviceEvent::Error { message } => {
                    state.log(format!("[ERROR] {message}"));
                }
            }
        }
    })
}

/// Import from the platform's legacy config directory into the app state.
pub fn run_import(state: &AppState, store: &ConfigStore) -> Result<ImportReport, ImportError> {
    run_import_from_legacy_dir(state, store, &legacy_config_dir())
}

/// Import from a specified legacy directory (also used for deterministic tests).
pub fn run_import_from_legacy_dir(
    state: &AppState,
    store: &ConfigStore,
    legacy_dir: &Path,
) -> Result<ImportReport, ImportError> {
    let report = import_legacy(store, legacy_dir)?;
    state.set_config(store.load_config(), "import");
    Ok(report)
}

/// Persist the current config before best-effort timing synchronization.
pub async fn save_config(state: &AppState, manager: &DeviceManager) -> bool {
    if !state.save() {
        return false;
    }
    let _ = manager.sync_timings(&state.config()).await;
    true
}

/// Build the current snapshot consumed by the future frontend.
pub fn state_snapshot(state: &AppState) -> Value {
    json!({
        "config": state.config(),
        "configPath": state.store().config_path().to_string_lossy().into_owned(),
        "modeIndex": state.mode_index(),
        "selectedIndex": state.selected_index(),
        "dirty": state.is_dirty(),
        "activePreset": state.active_preset(),
        "serial": {
            "connected": state.serial_connected(),
            "port": state.serial_port()
        },
        "ble": {
            "connected": state.ble_connected(),
            "message": state.ble_message()
        },
        "device": {
            "name": state.device_name(),
            "firmwareVersion": state.firmware_version()
        },
        "homeAssistant": {
            "connected": state.ha_connected(),
            "message": state.ha_message(),
            "entitiesCount": state.ha_entities_count()
        },
        "installedAppsCount": state.installed_apps_count()
    })
}
