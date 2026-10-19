use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use octopad_core::bridge::{connect_device_events, run_import_from_legacy_dir};
use octopad_core::config::{default_config, Action, ConfigStore, TriggerActions};
use octopad_core::device::manager::{DeviceManager, DeviceTransport};
use octopad_core::device::{DeviceEvent, TransportKind};
use octopad_core::state::{AppEvent, AppState};
use serde_json::{json, Value};
use tempfile::tempdir;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

#[derive(Clone, Default)]
struct EventRecorder(Arc<Mutex<Vec<AppEvent>>>);

impl EventRecorder {
    fn events(&self) -> Vec<AppEvent> {
        self.0.lock().unwrap().clone()
    }

    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }

    fn callback(&self) -> impl Fn(AppEvent) + Send + Sync + 'static {
        let events = self.0.clone();
        move |event| events.lock().unwrap().push(event)
    }
}

fn state_in(path: &std::path::Path) -> Arc<AppState> {
    Arc::new(AppState::new(ConfigStore::at(path.join("OctoPad"))))
}

// ---------------------------------------------------------------------------
// Injectable transports (mirrors `tests/device_manager.rs`, plus an on-disk
// probe so a test can prove *when* a command left the app)
// ---------------------------------------------------------------------------

/// Records every command routed to it as `(command, probe_existed_on_send)`.
///
/// The probe answer is what pins the save-before-sync ordering: the config file
/// is only on disk once the store write has completed.
#[derive(Clone)]
struct RecordingTransport {
    connected: bool,
    sends: Arc<Mutex<Vec<(String, bool)>>>,
    probe: Option<PathBuf>,
    events: Vec<DeviceEvent>,
}

impl RecordingTransport {
    fn new(connected: bool, events: Vec<DeviceEvent>, probe: Option<PathBuf>) -> Self {
        Self {
            connected,
            sends: Arc::new(Mutex::new(Vec::new())),
            probe,
            events,
        }
    }

    fn sends(&self) -> Vec<(String, bool)> {
        self.sends.lock().unwrap().clone()
    }
}

impl DeviceTransport for RecordingTransport {
    fn send_command<'a>(
        &'a self,
        command: &'a str,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            if !self.connected {
                return false;
            }
            let probe_existed = self.probe.as_deref().is_some_and(Path::exists);
            self.sends
                .lock()
                .unwrap()
                .push((command.to_string(), probe_existed));
            true
        })
    }

    fn spawn(
        &self,
        tx: mpsc::Sender<DeviceEvent>,
        mut shutdown: watch::Receiver<bool>,
    ) -> JoinHandle<()> {
        let events = self.events.clone();
        tokio::spawn(async move {
            for event in events {
                if tx.send(event).await.is_err() {
                    return;
                }
            }
            if !*shutdown.borrow() {
                let _ = shutdown.changed().await;
            }
        })
    }
}

struct RunningManager {
    manager: DeviceManager,
    events: mpsc::Receiver<DeviceEvent>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

fn status(transport: TransportKind, connected: bool) -> DeviceEvent {
    DeviceEvent::StatusChanged {
        transport,
        connected,
        port: None,
        message: None,
    }
}

fn start_manager(serial: RecordingTransport, ble: RecordingTransport) -> RunningManager {
    let manager = DeviceManager::with_transports(Arc::new(serial), Arc::new(ble));
    let (event_tx, events) = mpsc::channel(32);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let task = manager.spawn(event_tx, shutdown_rx);
    RunningManager {
        manager,
        events,
        shutdown,
        task,
    }
}

async fn stop_manager(running: RunningManager) {
    running.shutdown.send(true).unwrap();
    running.task.await.unwrap();
}

/// Drain until `transport` reports itself connected, so a later assertion is
/// not racing the manager's own status merge.
async fn wait_for_connected(events: &mut mpsc::Receiver<DeviceEvent>, transport: TransportKind) {
    let expected = status(transport, true);
    while let Some(event) = events.recv().await {
        if event == expected {
            return;
        }
    }
    panic!("manager stopped before reporting a connected {transport:?} transport");
}

#[tokio::test]
async fn device_events_update_state_and_emit_serial_logs() {
    let temp = tempdir().unwrap();
    let state = state_in(temp.path());
    let recorder = EventRecorder::default();
    let _subscription = state.subscribe(recorder.callback());
    let (tx, rx) = mpsc::channel(16);
    let task = connect_device_events(state.clone(), rx);

    tx.send(DeviceEvent::StatusChanged {
        transport: TransportKind::Serial,
        connected: true,
        port: Some("/dev/ttyUSB0".to_string()),
        message: None,
    })
    .await
    .unwrap();
    tx.send(DeviceEvent::StatusChanged {
        transport: TransportKind::Ble,
        connected: false,
        port: None,
        message: Some("Searching for BLE pad…".to_string()),
    })
    .await
    .unwrap();
    tx.send(DeviceEvent::HelloReceived {
        name: Some("My Pad".to_string()),
        version: "2.6".to_string(),
    })
    .await
    .unwrap();
    tx.send(DeviceEvent::Log {
        line: "HELLO:OCTOPAD:My Pad:v2.6".to_string(),
    })
    .await
    .unwrap();
    tx.send(DeviceEvent::ButtonEvent {
        // B2's `double` slot is `none` in the default config, so the chain this
        // press now spawns stays a no-op: the default config's `single` slots
        // hold real system actions that must not run inside a test.
        mode: "desktop".to_string(),
        index: 1,
        trigger: "double".to_string(),
    })
    .await
    .unwrap();
    tx.send(DeviceEvent::Error {
        message: "USB read failed".to_string(),
    })
    .await
    .unwrap();
    drop(tx);
    task.await.unwrap();

    assert!(state.serial_connected());
    assert_eq!(state.serial_port().as_deref(), Some("/dev/ttyUSB0"));
    assert!(!state.ble_connected());
    assert_eq!(state.ble_message(), "Searching for BLE pad…");
    assert_eq!(state.device_name().as_deref(), Some("My Pad"));
    assert_eq!(state.firmware_version().as_deref(), Some("2.6"));

    assert_eq!(
        recorder.events(),
        vec![
            AppEvent::SerialStatusChanged {
                connected: true,
                port: Some("/dev/ttyUSB0".to_string())
            },
            AppEvent::BleStatusChanged {
                connected: false,
                message: "Searching for BLE pad…".to_string()
            },
            AppEvent::DeviceNameChanged {
                name: Some("My Pad".to_string())
            },
            AppEvent::DeviceInfoChanged {
                version: Some("2.6".to_string())
            },
            AppEvent::SerialLog {
                line: "HELLO:OCTOPAD:My Pad:v2.6".to_string()
            },
            AppEvent::SerialLog {
                line: "[DESKTOP] Button 2 -> DOUBLE".to_string()
            },
            AppEvent::SerialLog {
                line: "[ERROR] USB read failed".to_string()
            },
        ]
    );
}

/// M3.1: a press no longer only logs — it runs the configured chain on a
/// blocking task. An empty `text` payload fails inside the dispatcher before any
/// injector is touched, so the assertion is end-to-end but side-effect free.
#[tokio::test]
async fn a_button_event_executes_the_configured_action_chain() {
    let temp = tempdir().unwrap();
    let state = state_in(temp.path());
    let mut config = state.config();
    config.modes[0].buttons[2].single = TriggerActions::from_list(vec![Action {
        action_type: "text".to_string(),
        extra: HashMap::from([("text".to_string(), json!(""))]),
    }]);
    state.set_config(config, "test");
    let recorder = EventRecorder::default();
    let _subscription = state.subscribe(recorder.callback());
    let (tx, rx) = mpsc::channel(16);
    let task = connect_device_events(state.clone(), rx);

    tx.send(DeviceEvent::ButtonEvent {
        mode: "desktop".to_string(),
        index: 2,
        trigger: "single".to_string(),
    })
    .await
    .unwrap();
    drop(tx);
    task.await.unwrap();

    // The chain runs on a blocking task, so its lines can land after the bridge
    // loop is done: wait for the result line instead of assuming an order.
    let started = Instant::now();
    let lines = loop {
        let lines: Vec<String> = recorder
            .events()
            .iter()
            .filter_map(|event| match event {
                AppEvent::SerialLog { line } => Some(line.clone()),
                _ => None,
            })
            .collect();
        if lines.iter().any(|line| line.starts_with("[ACTION]")) {
            break lines;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the chain never logged: {lines:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    };

    assert_eq!(
        lines,
        vec![
            "[DESKTOP] Button 3 -> SINGLE".to_string(),
            "Executing action for Button 3 (single) -> Type: text".to_string(),
            "[ACTION] B3 (SINGLE): Empty text [FAIL]".to_string(),
        ]
    );
}

#[tokio::test]
async fn corrupt_legacy_import_keeps_config_and_state_events_untouched() {
    let temp = tempdir().unwrap();
    let store = ConfigStore::at(temp.path().join("OctoPad"));
    let initial_state = AppState::new(store.clone());
    let mut initial_config = initial_state.config();
    initial_config.modes[0].buttons[0].label = "Saved label".to_string();
    store.save_config(&initial_config).unwrap();
    let state = Arc::new(AppState::new(store.clone()));
    let recorder = EventRecorder::default();
    let _subscription = state.subscribe(recorder.callback());
    let legacy_dir = temp.path().join("legacy");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    std::fs::write(legacy_dir.join("config.json"), "{broken").unwrap();

    let result = run_import_from_legacy_dir(&state, &store, &legacy_dir);

    assert!(matches!(
        result,
        Err(octopad_core::config::ImportError::Parse(_))
    ));
    assert_eq!(state.config().modes[0].buttons[0].label, "Saved label");
    assert!(recorder.events().is_empty());
}

#[tokio::test]
async fn successful_legacy_import_loads_working_config_without_saving() {
    let temp = tempdir().unwrap();
    let store = ConfigStore::at(temp.path().join("OctoPad"));
    let mut saved_config = default_config();
    saved_config.modes[0].buttons[0].label = "Saved label".to_string();
    store.save_config(&saved_config).unwrap();
    let state = Arc::new(AppState::new(store.clone()));
    let recorder = EventRecorder::default();
    let _subscription = state.subscribe(recorder.callback());
    let legacy_dir = temp.path().join("legacy");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    let mut legacy_config = default_config();
    legacy_config.modes[0].buttons[0].label = "Imported label".to_string();
    std::fs::write(
        legacy_dir.join("config.json"),
        serde_json::to_vec(&legacy_config).unwrap(),
    )
    .unwrap();

    let report = run_import_from_legacy_dir(&state, &store, &legacy_dir).unwrap();

    assert!(report.imported_config);
    assert_eq!(state.config().modes[0].buttons[0].label, "Imported label");
    assert!(state.is_dirty());
    assert_eq!(
        store.load_config().modes[0].buttons[0].label,
        "Saved label",
        "explicit import must not overwrite the saved config"
    );
    assert_eq!(
        recorder.events(),
        vec![
            AppEvent::ConfigReplaced {
                source: "import".to_string()
            },
            AppEvent::DirtyChanged { dirty: true }
        ]
    );
}

#[test]
fn state_snapshot_contains_config_and_connection_statuses() {
    let temp = tempdir().unwrap();
    let state = state_in(temp.path());

    let snapshot = octopad_core::bridge::state_snapshot(&state);

    assert_eq!(snapshot["config"]["schemaVersion"], Value::from(1));
    assert_eq!(snapshot["serial"]["connected"], Value::from(false));
    assert_eq!(snapshot["ble"]["connected"], Value::from(false));
    assert_eq!(snapshot["device"]["name"], Value::Null);
}

#[test]
fn state_snapshot_exposes_the_config_path() {
    let temp = tempdir().unwrap();
    let state = state_in(temp.path());

    let snapshot = octopad_core::bridge::state_snapshot(&state);

    assert_eq!(
        snapshot["configPath"],
        Value::from(state.store().config_path().to_string_lossy().to_string())
    );
}

// ---------------------------------------------------------------------------
// bridge::save_config — persist first, device sync only afterwards
// ---------------------------------------------------------------------------

#[tokio::test]
async fn save_config_persists_before_syncing_timings_and_clears_dirty() {
    let temp = tempdir().unwrap();
    let store = ConfigStore::at(temp.path().join("OctoPad"));
    let state = AppState::new(store.clone());
    let recorder = EventRecorder::default();
    let _subscription = state.subscribe(recorder.callback());
    state.set_timing("debounceMs", 40);
    state.set_timing("clickTimeoutMs", 260);
    state.set_timing("holdTimeoutMs", 460);
    assert!(state.is_dirty());

    // Only BLE is connected: the manager's automatic post-connect serial push
    // cannot be confused with the sync this save performs.
    let serial = RecordingTransport::new(false, vec![status(TransportKind::Serial, false)], None);
    let ble = RecordingTransport::new(
        true,
        vec![status(TransportKind::Ble, true)],
        Some(store.config_path()),
    );
    let mut running = start_manager(serial.clone(), ble.clone());
    wait_for_connected(&mut running.events, TransportKind::Ble).await;
    recorder.clear();

    assert!(octopad_core::bridge::save_config(&state, &running.manager).await);

    // `true` in the probe column: the config file was already on disk when the
    // transport received the command, i.e. the write precedes the sync.
    assert_eq!(ble.sends(), vec![("TIMING:40,260,460".to_string(), true)]);
    assert!(serial.sends().is_empty());
    assert!(!state.is_dirty());
    assert!(!state.has_changes());
    assert_eq!(
        recorder.events(),
        vec![
            AppEvent::DirtyChanged { dirty: false },
            AppEvent::ConfigSaved {
                ok: true,
                preset: None
            }
        ]
    );
    assert_eq!(store.load_config().debounce_ms, json!(40));
    stop_manager(running).await;
}

#[tokio::test]
async fn save_config_succeeds_when_no_transport_is_connected() {
    let temp = tempdir().unwrap();
    let store = ConfigStore::at(temp.path().join("OctoPad"));
    let state = AppState::new(store.clone());
    state.set_label("Offline Edit");

    // Never spawned, so no transport can report itself connected.
    let manager = DeviceManager::with_transports(
        Arc::new(RecordingTransport::new(false, Vec::new(), None)),
        Arc::new(RecordingTransport::new(false, Vec::new(), None)),
    );

    assert!(octopad_core::bridge::save_config(&state, &manager).await);
    assert!(!state.is_dirty());
    assert!(!state.has_changes());
    assert_eq!(
        store.load_config().modes[0].buttons[0].label,
        "Offline Edit"
    );
}

#[tokio::test]
async fn save_config_never_syncs_timings_when_the_storage_write_fails() {
    let temp = tempdir().unwrap();
    let store = ConfigStore::at(temp.path().join("OctoPad"));
    // A directory at the config path makes every store write fail.
    std::fs::create_dir_all(store.config_path()).unwrap();
    let state = AppState::new(store.clone());
    state.set_timing("debounceMs", 40);
    assert!(state.is_dirty());

    let serial = RecordingTransport::new(false, vec![status(TransportKind::Serial, false)], None);
    let ble = RecordingTransport::new(
        true,
        vec![status(TransportKind::Ble, true)],
        Some(store.config_path()),
    );
    let mut running = start_manager(serial.clone(), ble.clone());
    wait_for_connected(&mut running.events, TransportKind::Ble).await;

    assert!(!octopad_core::bridge::save_config(&state, &running.manager).await);
    assert!(
        ble.sends().is_empty(),
        "a failed save must not sync timings"
    );
    assert!(serial.sends().is_empty());
    assert!(
        state.is_dirty(),
        "a failed save must leave the working config dirty"
    );
    stop_manager(running).await;
}
