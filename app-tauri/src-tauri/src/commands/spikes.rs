//! M0 spike commands (Task 4): serial enumeration, the minimal HELLO probe
//! and the legacy config path.
//!
//! Exposed to the frontend so the owner can drive the spikes from the app or
//! devtools when collecting the measurements for `m0-report.md`.

use octopad_core::config_paths::legacy_config_dir;
use octopad_core::serial_spike::{self, Hello, SerialError};

/// The legacy OctoPad config directory, computed from platform defaults
/// (`~/.config/octopad` on Linux, `%APPDATA%\OctoPad` on Windows). Path
/// computation only — a missing directory never fails (Review Focus Input 3).
#[tauri::command]
pub fn spike_legacy_config_dir() -> String {
    legacy_config_dir().to_string_lossy().to_string()
}

/// Enumerate attached serial ports. An empty list (nothing attached) is a
/// successful result, never an error (Review Focus Input 2).
#[tauri::command]
pub fn spike_list_ports() -> Result<Vec<String>, String> {
    serial_spike::list_ports()
}

/// Minimal HELLO probe on one port: open at 115200, ask the pad to identify
/// itself, read until `HELLO:OCTOPAD:...` or the timeout, release the port.
///
/// - `Ok(None)` — the port does not exist (skip) or no HELLO arrived;
/// - `Err(SerialError)` — structured failure; `Busy` means another client
///   (e.g. the legacy Python app) already owns the port (Review Focus
///   Input 1), `PermissionDenied` means the user may not open it.
#[tauri::command]
pub fn spike_probe_hello(port_path: String, timeout_ms: u64) -> Result<Option<Hello>, SerialError> {
    serial_spike::probe_hello(&port_path, timeout_ms)
}
