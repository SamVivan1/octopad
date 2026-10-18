//! Initial M1 device/config commands exposed to the frontend.

use std::sync::Arc;

use octopad_core::bridge;
use octopad_core::config::{Action, ImportError, ImportReport};
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
pub async fn save_config_cmd(
    state: State<'_, Arc<AppState>>,
    manager: State<'_, DeviceManager>,
) -> bool {
    bridge::save_config(state.inner().as_ref(), manager.inner()).await
}

#[tauri::command]
pub async fn send_device_command_cmd(command: String, manager: State<'_, DeviceManager>) -> bool {
    manager.send_command(&command, true).await
}

#[tauri::command]
pub fn set_mode_cmd(index: i64, state: State<'_, Arc<AppState>>) -> bool {
    state.set_mode(index)
}

#[tauri::command]
pub fn select_button_cmd(index: i64, state: State<'_, Arc<AppState>>) -> bool {
    state.select_button(index)
}

#[tauri::command]
pub fn set_label_cmd(text: String, state: State<'_, Arc<AppState>>) {
    state.set_label(&text);
}

#[tauri::command]
pub fn set_action_cmd(
    trigger: String,
    index: i64,
    action: Action,
    state: State<'_, Arc<AppState>>,
) {
    state.set_action(&trigger, index, action);
}

#[tauri::command]
pub fn add_action_cmd(trigger: String, state: State<'_, Arc<AppState>>) {
    state.add_action(&trigger, None);
}

#[tauri::command]
pub fn remove_action_cmd(trigger: String, index: i64, state: State<'_, Arc<AppState>>) {
    state.remove_action(&trigger, index);
}

#[tauri::command]
pub fn move_action_cmd(trigger: String, index: i64, delta: i64, state: State<'_, Arc<AppState>>) {
    state.move_action(&trigger, index, delta);
}

#[tauri::command]
pub fn set_timing_cmd(field: String, value: i64, state: State<'_, Arc<AppState>>) -> bool {
    state.set_timing(&field, value)
}

#[tauri::command]
pub fn list_presets_cmd(state: State<'_, Arc<AppState>>) -> Vec<String> {
    state.store().list_presets()
}

#[tauri::command]
pub async fn rename_device_cmd(
    name: String,
    state: State<'_, Arc<AppState>>,
    manager: State<'_, DeviceManager>,
) -> bool {
    let name = name.trim();
    if !manager.rename_device(name).await {
        return false;
    }
    state.set_device_name(Some(name.to_string()));
    true
}
