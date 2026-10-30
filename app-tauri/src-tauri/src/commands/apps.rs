//! Tauri command surface: the desktop application scanner.
//!
//! The scan itself lives in `octopad_core::apps` (pure, injectable); this
//! command only wires it to the real production directories and publishes the
//! result over the bridge, the way every other command is a thin delegation.

use octopad_core::apps::{self, AppEntry};
use serde_json::json;
use tauri::{AppHandle, Emitter};

/// `octopad://apps-changed` — the spec contract carries the full list as
/// `apps: AppEntry[]` (`{"id", "name"}`), so an open picker refreshes without
/// re-invoking.
const APPS_CHANGED_EVENT: &str = "octopad://apps-changed";

/// Scan the standard application directories and return the visible apps.
///
/// The scan runs on a blocking task — a large applications directory must not
/// stall the async runtime — and the payload is emitted with the same
/// `octopad://` prefix the core's event bridge uses, so the frontend's single
/// `apps-changed` listener serves both sources.
///
/// The core's `AppState.set_apps` count-only event shape is deliberately still
/// tolerated by the store's handler (the read model's `installedAppsCount` is
/// patched from the returned list here instead), so `state.rs`'s existing
/// payload contract is untouched while the picker gets the full list.
#[tauri::command]
pub async fn list_applications_cmd(app: AppHandle) -> Result<Vec<AppEntry>, String> {
    let roots = apps::default_scan_roots();
    let found = tauri::async_runtime::spawn_blocking(move || apps::scan_applications(&roots))
        .await
        .map_err(|error| format!("application scan failed: {error}"))?;

    app.emit(APPS_CHANGED_EVENT, json!({ "apps": found }))
        .map_err(|error| format!("could not emit the application list: {error}"))?;
    Ok(found)
}
