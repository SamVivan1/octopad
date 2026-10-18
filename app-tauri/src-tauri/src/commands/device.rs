//! Initial M1 device/config commands exposed to the frontend.

use std::sync::Arc;

use octopad_core::bridge;
use octopad_core::config::{ImportError, ImportReport};
use octopad_core::device::manager::DeviceManager;
use octopad_core::state::AppState;
use serde_json::Value;
use tauri::State;

#[tauri::command]
pub fn get_state_snapshot(state: State<'_, Arc<AppState>>) -> Value {
    bridge::state_snapshot(state.inner().as_ref())
}

#[tauri::command]
pub fn import_legacy_config(state: State<'_, Arc<AppState>>) -> Result<ImportReport, ImportError> {
    bridge::run_import(state.inner().as_ref(), state.store())
}

#[tauri::command]
pub fn save_config_cmd(state: State<'_, Arc<AppState>>) -> bool {
    state.save()
}

#[tauri::command]
pub fn list_presets_cmd(state: State<'_, Arc<AppState>>) -> Vec<String> {
    state.store().list_presets()
}

#[tauri::command]
pub fn rename_device_cmd(
    name: String,
    state: State<'_, Arc<AppState>>,
    manager: State<'_, DeviceManager>,
) -> bool {
    let name = name.trim();
    if !manager.rename_device(name) {
        return false;
    }
    state.set_device_name(Some(name.to_string()));
    true
}
