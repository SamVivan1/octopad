//! Tauri command layer — the backend surface exposed to the frontend via
//! `invoke_handler` (see `crate::lib.rs`).

use tauri::AppHandle;

pub mod device;
pub mod spikes;

/// Quit for real: signal runtime shutdown and exit.
///
/// Backs the `Ctrl+Q` shortcut; the tray Quit item calls the same helper
/// directly. Closing the window is *not* this command — that only hides the
/// window, deliberately keeping the device connections alive (Global Constraint:
/// closing the window hides it to the tray, Quit exits).
#[tauri::command]
pub fn quit_app_cmd(app: AppHandle) {
    crate::shutdown_and_exit(&app);
}
