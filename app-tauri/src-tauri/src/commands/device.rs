//! Tauri command surface: snapshot, config lifecycle, state edits and device
//! I/O.
//!
//! Every body is a one-line delegation — validation, bounds checking and dirty
//! tracking live in `octopad-core`, so the UI can trust a `false` return to mean
//! "nothing changed" (Global Constraint: a failed command must never be
//! reported as a successful edit or send).

use std::sync::Arc;

use octopad_core::bridge;
use octopad_core::config::{Action, ImportError, ImportReport};
use octopad_core::device::manager::DeviceManager;
use octopad_core::state::AppState;
use serde_json::Value;
use tauri::{AppHandle, Manager, State};

// -- snapshot / config lifecycle ---------------------------------------------

#[tauri::command]
pub fn get_state_snapshot(state: State<'_, Arc<AppState>>) -> Value {
    bridge::state_snapshot(state.inner().as_ref())
}

#[tauri::command]
pub fn import_legacy_config(state: State<'_, Arc<AppState>>) -> Result<ImportReport, ImportError> {
    bridge::run_import(state.inner().as_ref(), state.store())
}

/// Persist the working config, then best-effort sync timings to the pad.
///
/// Async commands cannot take `State<'_, _>`: the generated future must be
/// `'static`, and Tauri additionally requires a `Result` from async commands
/// that receive borrowed arguments. Passing an owned `AppHandle` and resolving
/// the managed state inside the body satisfies both rules while keeping the
/// command body a thin delegation.
#[tauri::command]
pub async fn save_config_cmd(app: AppHandle) -> bool {
    let Some(state) = app.try_state::<Arc<AppState>>() else {
        return false;
    };
    let Some(manager) = app.try_state::<DeviceManager>() else {
        return false;
    };
    bridge::save_config(state.inner().as_ref(), manager.inner()).await
}

#[tauri::command]
pub fn list_presets_cmd(state: State<'_, Arc<AppState>>) -> Vec<String> {
    state.store().list_presets()
}

// -- navigation --------------------------------------------------------------

/// Switch mode; `false` when the index is outside the configured modes.
#[tauri::command]
pub fn set_mode_cmd(index: i64, state: State<'_, Arc<AppState>>) -> bool {
    state.set_mode(index)
}

/// Select the edited button; `false` when it is outside the current mode.
#[tauri::command]
pub fn select_button_cmd(index: i64, state: State<'_, Arc<AppState>>) -> bool {
    state.select_button(index)
}

// -- button / action edits ---------------------------------------------------

#[tauri::command]
pub fn set_label_cmd(text: String, state: State<'_, Arc<AppState>>) {
    state.set_label(&text);
}

/// Replace an action; `false` unless `trigger` is `single`, `double` or `hold`.
#[tauri::command]
pub fn set_action_cmd(
    trigger: String,
    index: i64,
    action: Action,
    state: State<'_, Arc<AppState>>,
) -> bool {
    state.set_action(&trigger, index, action)
}

/// Append a `none` action; `false` for an unknown trigger.
#[tauri::command]
pub fn add_action_cmd(trigger: String, state: State<'_, Arc<AppState>>) -> bool {
    state.add_action(&trigger, None)
}

/// Remove an action; `false` for an unknown trigger.
#[tauri::command]
pub fn remove_action_cmd(trigger: String, index: i64, state: State<'_, Arc<AppState>>) -> bool {
    state.remove_action(&trigger, index)
}

/// Reorder an action; `false` for an unknown trigger.
#[tauri::command]
pub fn move_action_cmd(
    trigger: String,
    index: i64,
    delta: i64,
    state: State<'_, Arc<AppState>>,
) -> bool {
    state.move_action(&trigger, index, delta)
}

/// Set `debounceMs` / `clickTimeoutMs` / `holdTimeoutMs`; `false` for any
/// other field name.
#[tauri::command]
pub fn set_timing_cmd(field: String, value: i64, state: State<'_, Arc<AppState>>) -> bool {
    state.set_timing(&field, value)
}

// -- device ------------------------------------------------------------------

/// Rename the pad; `false` (and no name change) when the name is rejected or
/// the transport write fails. Takes an owned `AppHandle` for the same reason
/// as [`save_config_cmd`].
#[tauri::command]
pub async fn rename_device_cmd(name: String, app: AppHandle) -> bool {
    let name = name.trim();
    let Some(manager) = app.try_state::<DeviceManager>() else {
        return false;
    };
    if !manager.rename_device(name).await {
        return false;
    }
    let Some(state) = app.try_state::<Arc<AppState>>() else {
        return false;
    };
    state.set_device_name(Some(name.to_string()));
    true
}

/// Send a raw console command; `true` only once a transport reports a
/// successful write.
#[tauri::command]
pub async fn send_device_command_cmd(command: String, app: AppHandle) -> bool {
    let Some(manager) = app.try_state::<DeviceManager>() else {
        return false;
    };
    manager.send_command(&command, true).await
}
