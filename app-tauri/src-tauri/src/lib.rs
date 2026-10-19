//! OctoPad desktop app: window, tray residency, device runtime and commands.
//!
//! M2 residency rules, and where they live:
//!
//! - **Closing the window hides it.** `WindowEvent::CloseRequested` is
//!   prevented and the window is hidden, so serial/BLE connections keep running
//!   with no window on screen. Only [`shutdown_and_exit`] really ends the app.
//! - **The tray is the resident surface.** Its menu carries Open, a live
//!   connection-status line driven by the same events the frontend listens to,
//!   and Quit. The tray *icon* is the real OctoPad artwork in three states
//!   (connected / checking / off), picked from those same events.
//! - **A second launch never starts a second runtime.** The single-instance
//!   plugin is registered first in the builder chain and asks the resident
//!   instance to show and focus its window — unless the second launch carries
//!   `--hidden`, which is an autostart re-fire and must not steal focus.
//! - **`--hidden` starts in the tray.** That is the argument the autostart login
//!   entry carries. The window is configured `visible: false` and only shown
//!   from `setup` when the argument is absent, so an autostart launch never
//!   maps — and therefore never flashes — a window.

mod commands;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use octopad_core::bridge;
use octopad_core::config::{auto_import_legacy, ConfigStore};
use octopad_core::device::manager::DeviceManager;
use octopad_core::state::{AppState, Subscription};
use serde_json::Value;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::path::BaseDirectory;
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::{mpsc, watch};

/// Tray icon id, and the ids of its menu items.
const TRAY_ID: &str = "octopad-tray";
const MENU_OPEN: &str = "open";
const MENU_STATUS: &str = "status";
const MENU_QUIT: &str = "quit";

/// The main window's label; the tray and the single-instance handler address it
/// by name so there is exactly one place that knows it.
const MAIN_WINDOW: &str = "main";

/// Argument the OS autostart entry passes so a login launch stays in the tray.
const HIDDEN_ARG: &str = "--hidden";

/// Shutdown control shared by the app, the tray Quit item and `Ctrl+Q`.
struct RuntimeControl {
    shutdown: watch::Sender<bool>,
    /// Set before an intentional exit so the close-to-tray path stands down:
    /// an exiting app must not be told to hide a window it is about to lose.
    quitting: AtomicBool,
}

/// Connection state as the tray reports it, mirrored from the same events the
/// frontend receives, so the resident app still shows its state with no window.
#[derive(Clone, Default)]
struct ConnectionStatus {
    serial: bool,
    ble: bool,
    /// Last BLE status line. It is the only place the core says a scan or a
    /// reconnect is in flight (`"Searching for BLE pad…"`,
    /// `"Pad disconnected — retrying…"`); serial disconnects carry no message.
    ble_message: String,
}

impl ConnectionStatus {
    /// Tray menu text. Short enough for a system menu, and never optimistic.
    fn label(&self) -> String {
        let text = match (self.serial, self.ble) {
            (true, true) => "Serial + Bluetooth",
            (true, false) => "Serial",
            (false, true) => "Bluetooth",
            (false, false) => "Not connected",
        };
        format!("Status: {text}")
    }

    /// Fold one device-status event in. Returns false for every event that does
    /// not carry a connection flag — the tray has nothing to say about those.
    fn apply(&mut self, name: &str, payload: &Value) -> bool {
        let Some(connected) = connection_flag(payload) else {
            return false;
        };
        match name {
            "serial-status-changed" => self.serial = connected,
            "ble-status-changed" => {
                self.ble = connected;
                self.ble_message = payload
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
            _ => {}
        }
        true
    }

    /// Which of the three bundled tray icons this connection state shows.
    ///
    /// Python parity (`linux-app/tray.py::_device_state`): either transport up
    /// wins over the message, because a live connection is never "checking".
    fn tray_state(&self) -> TrayState {
        if self.serial || self.ble {
            return TrayState::Connected;
        }
        let message = self.ble_message.to_lowercase();
        if message.contains("searching") || message.contains("retry") {
            TrayState::Checking
        } else {
            TrayState::Off
        }
    }
}

/// The three connection states the tray artwork exists for.
///
/// The assets are the `linux-app` tray icons shipped as-is under
/// `resources/tray/` and installed through `bundle.resources`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrayState {
    /// Serial and/or BLE is up: the green pad.
    Connected,
    /// Nothing is up but a scan or reconnect is running: the amber pad.
    Checking,
    /// Nothing is up and nothing is being tried: the red pad.
    Off,
}

impl TrayState {
    /// Path inside the Tauri resource directory. `BaseDirectory::Resource` maps
    /// to `bundle.resources` in `tauri.conf.json`.
    const fn resource(self) -> &'static str {
        match self {
            TrayState::Connected => "tray/octopad-tray.png",
            TrayState::Checking => "tray/octopad-tray-checking.png",
            TrayState::Off => "tray/octopad-tray-off.png",
        }
    }
}

/// The three tray icons, decoded once at start-up.
///
/// `Image` owns its pixels (`Cow::Owned`), so a state change is a clone and a
/// re-decode never happens while the app runs — no file is reopened per event
/// and no handle can be left open.
struct TrayIcons {
    connected: Image<'static>,
    checking: Image<'static>,
    off: Image<'static>,
}

impl TrayIcons {
    fn get(&self, state: TrayState) -> &Image<'static> {
        match state {
            TrayState::Connected => &self.connected,
            TrayState::Checking => &self.checking,
            TrayState::Off => &self.off,
        }
    }
}

/// Everything the tray subscription mutates, behind one lock.
struct TrayStatus {
    /// The non-clickable menu line reporting the connection.
    status: MenuItem<tauri::Wry>,
    /// `None` only when the resources could not be loaded, in which case the
    /// tray keeps the bundled window icon instead of going blank.
    icons: Option<TrayIcons>,
    connection: ConnectionStatus,
    /// The state the icon is *currently showing*, so an event that does not
    /// change it cannot rebuild the icon.
    shown: Option<TrayState>,
}

/// `connected` out of a device-status payload, if it carries one.
fn connection_flag(payload: &Value) -> Option<bool> {
    payload.get("connected").and_then(Value::as_bool)
}

/// Path of a resource inside the source tree, used by `tauri dev` (which runs
/// from `target/`, where nothing is installed) and by the asset test.
fn source_resource_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join(relative)
}

/// Where a bundled resource lives on disk.
///
/// The bundle installs `resources/tray/*` into the resource directory, which on
/// Linux is `/usr/lib/OctoPad` (next to the `/usr/bin/octopad-tauri` binary).
/// `tauri dev` runs from `target/`, where nothing is installed, so it falls back
/// to the source tree — otherwise the tray would show the app icon for the whole
/// dev session.
fn resource_path<R: Runtime>(app: &AppHandle<R>, relative: &str) -> Option<PathBuf> {
    let installed = app.path().resolve(relative, BaseDirectory::Resource).ok();
    if let Some(path) = installed.filter(|path| path.is_file()) {
        return Some(path);
    }
    let source = source_resource_path(relative);
    source.is_file().then_some(source)
}

/// Decode all three tray icons. Called exactly once, from `build_tray`.
fn load_tray_icons<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<TrayIcons> {
    let load = |state: TrayState| -> tauri::Result<Image<'static>> {
        let path = resource_path(app, state.resource())
            .ok_or_else(|| tauri::Error::AssetNotFound(state.resource().to_string()))?;
        // `from_path` reads the file in full and closes it; the decoded pixels
        // are owned by the returned `Image`.
        Image::from_path(&path)
    };
    Ok(TrayIcons {
        connected: load(TrayState::Connected)?,
        checking: load(TrayState::Checking)?,
        off: load(TrayState::Off)?,
    })
}

/// Bring the main window back, whatever state the tray left it in.
fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// End the app for real: stop the device runtime, then exit.
///
/// The single exit path — the tray Quit item and `quit_app_cmd` (`Ctrl+Q`) both
/// call it, so quitting can never be reached by one and not the other.
fn shutdown_and_exit<R: Runtime>(app: &AppHandle<R>) {
    if let Some(runtime) = app.try_state::<RuntimeControl>() {
        runtime.quitting.store(true, Ordering::SeqCst);
        let _ = runtime.shutdown.send(true);
    }
    app.exit(0);
}

/// True once an intentional quit has begun.
fn is_quitting<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<RuntimeControl>()
        .map(|runtime| runtime.quitting.load(Ordering::SeqCst))
        .unwrap_or(false)
}

/// Build the tray: Open, a live status line, and Quit, with the "off" tray icon
/// until the core reports something better.
fn build_tray(app: &tauri::App) -> tauri::Result<Arc<Mutex<TrayStatus>>> {
    let open = MenuItem::with_id(app, MENU_OPEN, "Open", true, None::<&str>)?;
    // Not clickable: it is a report of what the core last told us.
    let status = MenuItem::with_id(
        app,
        MENU_STATUS,
        ConnectionStatus::default().label(),
        false,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &status, &quit])?;

    // Decoded once. A missing resource must not take the app down: the tray
    // falls back to the bundled window icon and says so on stderr.
    let icons = match load_tray_icons(app.handle()) {
        Ok(icons) => Some(icons),
        Err(error) => {
            eprintln!("tray icons unavailable, falling back to the app icon: {error}");
            None
        }
    };

    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("OctoPad")
        .menu(&menu)
        // Linux/GTK convention here: the left button opens the app and the menu
        // opens on right click (on Windows the left click shows the menu).
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            MENU_OPEN => show_main_window(app),
            MENU_QUIT => shutdown_and_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    // Nothing is connected at start-up, so start on the "off" artwork; the
    // bundled window icon is the fallback when the resources are missing.
    match icons.as_ref() {
        Some(icons) => tray = tray.icon(icons.get(TrayState::Off).clone()),
        None => {
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
        }
    }
    tray.build(app)?;

    let shown = icons.as_ref().map(|_| TrayState::Off);
    Ok(Arc::new(Mutex::new(TrayStatus {
        status,
        icons,
        connection: ConnectionStatus::default(),
        shown,
    })))
}

/// Mirror the core's device-status events into the tray: the status line and
/// the stateful icon, both from the same events the frontend listens to.
fn update_tray(
    tray: &TrayIcon<tauri::Wry>,
    shared: &Mutex<TrayStatus>,
    name: &str,
    payload: &Value,
) {
    // The icon is cloned under the lock and swapped in outside it, so the lock
    // is never held across a call into the toolkit.
    let icon = {
        let Ok(mut guard) = shared.lock() else {
            return;
        };
        if !guard.connection.apply(name, payload) {
            return;
        }
        let _ = guard.status.set_text(guard.connection.label());
        let Some(icons) = guard.icons.as_ref() else {
            return;
        };
        let state = guard.connection.tray_state();
        // No state change: leave the icon alone instead of rebuilding it.
        if guard.shown == Some(state) {
            return;
        }
        let icon = icons.get(state).clone();
        guard.shown = Some(state);
        Some(icon)
    };
    if let Some(icon) = icon {
        let _ = tray.set_icon(Some(icon));
    }
}

/// Close-to-tray: a closed window is a hidden window.
fn handle_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    if window.label() != MAIN_WINDOW {
        return;
    }
    let tauri::WindowEvent::CloseRequested { api, .. } = event else {
        return;
    };
    if is_quitting(window.app_handle()) {
        return;
    }
    // The runtime keeps running: hiding is what makes this app resident.
    api.prevent_close();
    let _ = window.hide();
}

#[tauri::command]
fn get_version() -> String {
    "0.1.0".to_string()
}

/// WebKit knobs that cut the app's real memory footprint.
///
/// Measured on this machine (Linux, fresh load, idle, worst of three samples per
/// process, via `scripts/mem_probe.sh`):
///
/// | configuration                   | RSS sum | PSS sum |
/// |---------------------------------|---------|---------|
/// | default                         | 515 MiB | 258 MiB |
/// | `WEBKIT_DISABLE_DMABUF_RENDERER`| 440 MiB | 193 MiB |
/// | both flags below                | 434 MiB | 185 MiB |
///
/// The RSS column overstates the cost: the app runs nine processes and RSS counts
/// pages shared between them more than once. PSS is the number to compare against
/// the legacy single-process Python app.
///
/// `LIBGL_ALWAYS_SOFTWARE=1` is deliberately NOT set: measured on its own it made
/// the app worse (PSS 526 MiB) because software GL rasterises into large CPU-side
/// buffers. It once looked marginally better inside a three-flag run, and three
/// repeats proved that was noise (209/200/194 MiB — no better than the two flags).
///
/// Existing values win, so a user can still override any of them.
fn apply_webview_memory_env() {
    const DEFAULTS: [(&str, &str); 2] = [
        // The DMA-BUF renderer is the single largest allocator in WebKitGTK.
        ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
        // Skips the extra compositing surfaces WebKit keeps per view.
        ("WEBKIT_DISABLE_COMPOSITING_MODE", "1"),
    ];
    for (key, value) in DEFAULTS {
        if std::env::var_os(key).is_none() {
            std::env::set_var(key, value);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Must run before Tauri initialises the webview: WebKitGTK reads these once
    // during startup, so setting them afterwards silently does nothing.
    apply_webview_memory_env();

    let app = tauri::Builder::default()
        // Registered first, before every other plugin: a second launch must be
        // intercepted before anything else initialises a second runtime.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // An autostart re-fire carries `--hidden` as well: it must not pull
            // the resident app's window back out of the tray.
            if !argv.iter().any(|argument| argument == HIDDEN_ARG) {
                // The resident instance already owns the device connections, so
                // the second launch only asks for the window back.
                show_main_window(app);
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("OctoPad")
                // The login entry starts the app in the tray; a manual launch has
                // no argument and shows the window as usual.
                .arg(HIDDEN_ARG)
                .build(),
        )
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

            let tray = build_tray(app)?;

            let app_handle = app.handle().clone();
            let subscription: Subscription = state.subscribe(move |event| {
                let (name, payload) = event.payload();
                let topic = format!("octopad://{name}");
                if let Err(error) = app_handle.emit(&topic, &payload) {
                    eprintln!("failed to emit app event {topic}: {error}");
                }
                if let Some(tray_icon) = app_handle.tray_by_id(TRAY_ID) {
                    update_tray(&tray_icon, &tray, name, &payload);
                }
            });

            if !app.manage(state.clone())
                || !app.manage(manager.clone())
                || !app.manage(RuntimeControl {
                    shutdown: shutdown.clone(),
                    quitting: AtomicBool::new(false),
                })
                || !app.manage(subscription)
            {
                return Err("failed to initialize app runtime state".into());
            }

            // Both of these start tokio work — `manager.spawn` a blocking task,
            // `connect_device_events` a future — and the setup hook runs on the
            // main thread with no reactor entered, where tokio panics with
            // "there is no reactor running". Entering Tauri's async runtime for
            // exactly these two calls is what starts the device runtime; the
            // work inside is a spawn each, so this returns immediately.
            tauri::async_runtime::block_on(async {
                manager.spawn(device_tx, shutdown_rx);
                bridge::connect_device_events(state.clone(), device_rx);
            });

            // The window is configured `visible: false`, so it is created (the
            // runtime and the tray need it) but never mapped before this point.
            // Show it here only for a normal launch; an autostart `--hidden`
            // launch leaves it unmapped until Open is used.
            if !std::env::args().any(|argument| argument == HIDDEN_ARG) {
                if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
                    let _ = window.show();
                }
            }
            Ok(())
        })
        .on_window_event(handle_window_event)
        .invoke_handler(tauri::generate_handler![
            get_version,
            commands::quit_app_cmd,
            commands::spikes::spike_legacy_config_dir,
            commands::spikes::spike_list_ports,
            commands::spikes::spike_probe_hello,
            commands::device::get_state_snapshot,
            commands::device::import_legacy_config,
            commands::device::save_config_cmd,
            commands::device::list_presets_cmd,
            commands::device::rename_device_cmd,
            commands::device::set_mode_cmd,
            commands::device::select_button_cmd,
            commands::device::set_label_cmd,
            commands::device::set_action_cmd,
            commands::device::add_action_cmd,
            commands::device::remove_action_cmd,
            commands::device::move_action_cmd,
            commands::device::set_timing_cmd,
            commands::device::send_device_command_cmd
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn serial(connected: bool) -> (&'static str, Value) {
        ("serial-status-changed", json!({ "connected": connected }))
    }

    fn ble(connected: bool, message: &str) -> (&'static str, Value) {
        (
            "ble-status-changed",
            json!({ "connected": connected, "message": message }),
        )
    }

    /// Drive `ConnectionStatus` the way the subscription does.
    fn fold(events: &[(&'static str, Value)]) -> ConnectionStatus {
        let mut status = ConnectionStatus::default();
        for (name, payload) in events {
            status.apply(name, payload);
        }
        status
    }

    /// The mapping the tray icon depends on, pinned against
    /// `linux-app/tray.py::_device_state`.
    #[test]
    fn no_transport_and_no_message_is_off() {
        assert_eq!(fold(&[]).tray_state(), TrayState::Off);
    }

    #[test]
    fn serial_only_is_connected() {
        let status = fold(&[serial(true)]);
        assert_eq!(status.tray_state(), TrayState::Connected);
        assert_eq!(status.label(), "Status: Serial");
    }

    #[test]
    fn ble_only_is_connected() {
        let status = fold(&[ble(true, "BLE pad connected")]);
        assert_eq!(status.tray_state(), TrayState::Connected);
        assert_eq!(status.label(), "Status: Bluetooth");
    }

    #[test]
    fn both_transports_are_connected() {
        let status = fold(&[serial(true), ble(true, "BLE pad connected")]);
        assert_eq!(status.tray_state(), TrayState::Connected);
        assert_eq!(status.label(), "Status: Serial + Bluetooth");
    }

    /// A scan that is still running is "checking", not "off" — the exact
    /// message the BLE transport emits (`ble_transport::STATUS_SEARCHING`).
    #[test]
    fn searching_is_checking() {
        let status = fold(&[ble(false, "Searching for BLE pad…")]);
        assert_eq!(status.tray_state(), TrayState::Checking);
        assert_eq!(status.label(), "Status: Not connected");
    }

    /// Same for the reconnect back-off (`STATUS_DISCONNECTED` carries "retry").
    #[test]
    fn retrying_is_checking() {
        let status = fold(&[ble(false, "Pad disconnected — retrying…")]);
        assert_eq!(status.tray_state(), TrayState::Checking);
    }

    #[test]
    fn search_matching_is_case_insensitive() {
        let status = fold(&[ble(false, "SEARCHING for ble pad")]);
        assert_eq!(status.tray_state(), TrayState::Checking);
    }

    /// A live connection always wins over a stale "searching" message.
    #[test]
    fn a_live_transport_beats_a_searching_message() {
        let status = fold(&[ble(false, "Searching for BLE pad…"), serial(true)]);
        assert_eq!(status.tray_state(), TrayState::Connected);
    }

    #[test]
    fn an_unrelated_disconnect_message_is_off() {
        let status = fold(&[ble(false, "Bluetooth adapter off")]);
        assert_eq!(status.tray_state(), TrayState::Off);
    }

    /// A serial disconnect puts the tray back to "off", not to a stale state.
    #[test]
    fn serial_disconnect_returns_to_off() {
        let status = fold(&[serial(true), serial(false)]);
        assert_eq!(status.tray_state(), TrayState::Off);
    }

    #[test]
    fn every_state_maps_to_its_own_asset() {
        assert_eq!(TrayState::Connected.resource(), "tray/octopad-tray.png");
        assert_eq!(
            TrayState::Checking.resource(),
            "tray/octopad-tray-checking.png"
        );
        assert_eq!(TrayState::Off.resource(), "tray/octopad-tray-off.png");
    }

    /// Events without a `connected` flag are ignored entirely — a config save
    /// must never move the tray icon.
    #[test]
    fn events_without_a_connection_flag_are_ignored() {
        let mut status = ConnectionStatus::default();
        assert!(!status.apply("config-saved", &json!({ "ok": true })));
        assert!(!status.apply("ble-status-changed", &json!({ "message": "hi" })));
        assert_eq!(status.tray_state(), TrayState::Off);
    }

    /// The three assets exist and decode: a blank or missing file is the exact
    /// regression this mapping would hide.
    #[test]
    fn the_three_tray_assets_are_present_and_decode() {
        for state in [TrayState::Connected, TrayState::Checking, TrayState::Off] {
            let path = source_resource_path(state.resource());
            let metadata = std::fs::metadata(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(metadata.len() > 0, "{} is empty", path.display());
            let image = Image::from_path(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!((image.width(), image.height()), (32, 32));
            assert_eq!(image.rgba().len(), 32 * 32 * 4);
        }
    }
}
