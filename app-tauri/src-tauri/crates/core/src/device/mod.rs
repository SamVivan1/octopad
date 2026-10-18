//! Device layer: transports that talk to the pad and the events they emit.
//!
//! The transports are deliberately decoupled from `AppState`: they push
//! [`DeviceEvent`]s into a `tokio::sync::mpsc` channel, and a later bridge maps
//! those onto state mutators (Task 8). Every transport shuts down cooperatively
//! via a `tokio::sync::watch` channel (Global Constraint: no extra crate).
//!
//! - [`protocol`] — pure protocol helpers (HELLO/event parsing, candidate-port
//!   filtering, BLE stream reassembly and device classification) plus the
//!   `serial`-gated port I/O helpers moved from the M0 `serial_spike` spike.
//! - [`serial_transport`] — USB serial with the Python reconnect loop
//!   (`serial_listener.py::_run_loop`): scan, probe, push timings, read lines,
//!   rescan 2s when no port/open failed and 1s after a mid-session read error.
//! - [`backoff`] — the shared exponential reconnect ladder (3s → ×2 capped
//!   15s, reset on connect).
//! - [`ble_transport`] — Bluetooth LE with the Python reconnect loop
//!   (`ble_bridge.py::_main`): scan by service UUID (name fallback), read TX
//!   once for the HELLO identity, reassemble notifications, write commands to
//!   RX and reconnect with the backoff above.

pub mod backoff;
pub mod protocol;

#[cfg(feature = "serial")]
pub mod serial_transport;

#[cfg(feature = "ble")]
pub mod ble_transport;

/// Which transport an event/status came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum TransportKind {
    /// USB serial (`serial_listener.py`).
    Serial,
    /// Bluetooth LE (`ble_bridge.py`).
    Ble,
}

/// Everything a transport reports to the rest of the core.
///
/// `StatusChanged` is transport-tagged so one event stream can carry both
/// serial and BLE status (the manager merges them, Task 7). Serial disconnects
/// leave `message` empty; BLE disconnects carry the human message the Python
/// bridge shows (`"Pad disconnected — retrying…"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceEvent {
    /// A raw log line (device → host lines, `[TX] …` command echoes, …).
    Log { line: String },
    /// Connection state for one transport.
    StatusChanged {
        transport: TransportKind,
        connected: bool,
        port: Option<String>,
        message: Option<String>,
    },
    /// The pad identified itself (`HELLO:OCTOPAD:<name>:v<version>`).
    HelloReceived {
        name: Option<String>,
        version: String,
    },
    /// A parsed button trigger (`[DESKTOP] Button 3 -> SINGLE`), 0-based index.
    ButtonEvent {
        mode: String,
        index: i64,
        trigger: String,
    },
    /// A structured transport error (e.g. BLE "device busy").
    Error { message: String },
}
