use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use octopad_core::bridge::{connect_device_events, run_import_from_legacy_dir, save_config};
use octopad_core::config::{default_config, ConfigStore};
use octopad_core::device::manager::{DeviceManager, DeviceTransport};
use octopad_core::device::{DeviceEvent, TransportKind};
use octopad_core::state::{AppEvent, AppState};
use serde_json::Value;
use tempfile::tempdir;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

#[derive(Clone, Default)]
struct EventRecorder(Arc<Mutex<Vec<AppEvent>>>);

impl EventRecorder {
    fn events(&self) -> Vec<AppEvent> {
        self.0.lock().unwrap().clone()
    }

    fn callback(&self) -> impl Fn(AppEvent) + Send + Sync + 'static {
        let events = self.0.clone();
        move |event| events.lock().unwrap().push(event)
    }
}

fn state_in(path: &std::path::Path) -> Arc<AppState> {
    Arc::new(AppState::new(ConfigStore::at(path.join("OctoPad"))))
}

#[derive(Clone)]
struct FakeTransport {
    status: Option<DeviceEvent>,
    commands: Arc<Mutex<Vec<String>>>,
}

impl FakeTransport {
    fn new(status: Option<DeviceEvent>, commands: Arc<Mutex<Vec<String>>>) -> Self {
        Self { status, commands }
    }
}

impl DeviceTransport for FakeTransport {
    fn spawn(
        &self,
        tx: mpsc::Sender<DeviceEvent>,
        mut shutdown: watch::Receiver<bool>,
    ) -> JoinHandle<()> {
        let status = self.status.clone();
        tokio::spawn(async move {
            if let Some(status) = status {
                let _ = tx.send(status).await;
            }
            if !*shutdown.borrow() {
                let _ = shutdown.changed().await;
            }
        })
    }

    fn send_command<'a>(
        &'a self,
        command: &'a str,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            self.commands.lock().unwrap().push(command.to_string());
            true
        })
    }
}

struct RunningManager {
    manager: DeviceManager,
    events: mpsc::Receiver<DeviceEvent>,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
    commands: Arc<Mutex<Vec<String>>>,
}

async fn start_manager(connected: bool) -> RunningManager {
    let commands = Arc::new(Mutex::new(Vec::new()));
    let status = connected.then_some(DeviceEvent::StatusChanged {
        transport: TransportKind::Ble,
        connected: true,
        port: None,
        message: Some("Connected".to_string()),
    });
    let serial = FakeTransport::new(None, commands.clone());
    let ble = FakeTransport::new(status, commands.clone());
    let manager = DeviceManager::with_transports(Arc::new(serial), Arc::new(ble));
    let (tx, events) = mpsc::channel(16);
    let (shutdown, shutdown_rx) = watch::channel(false);
    let task = manager.spawn(tx, shutdown_rx);
    let mut running = RunningManager {
        manager,
        events,
        shutdown,
        task,
        commands,
    };
    if connected {
        assert!(matches!(
            running.events.recv().await,
            Some(DeviceEvent::StatusChanged {
                transport: TransportKind::Ble,
                connected: true,
                ..
            })
        ));
    }
    running
}

async fn stop_manager(running: RunningManager) {
    running.shutdown.send(true).unwrap();
    running.task.await.unwrap();
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
        mode: "desktop".to_string(),
        index: 2,
        trigger: "single".to_string(),
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
                line: "[DESKTOP] Button 3 -> SINGLE".to_string()
            },
            AppEvent::SerialLog {
                line: "[ERROR] USB read failed".to_string()
            },
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
async fn successful_legacy_import_replaces_config_and_runs_dirty_flow() {
    let temp = tempdir().unwrap();
    let store = ConfigStore::at(temp.path().join("OctoPad"));
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
    assert_eq!(
        snapshot["configPath"],
        state.store().config_path().to_string_lossy().as_ref()
    );
}

#[tokio::test]
async fn save_config_persists_and_syncs_timings_without_requiring_a_device() {
    let temp = tempdir().unwrap();
    let state = state_in(temp.path());
    state.set_label("Unsaved");
    let running = start_manager(false).await;

    assert!(save_config(&state, &running.manager).await);
    assert!(!state.is_dirty());
    assert!(running.commands.lock().unwrap().is_empty());

    stop_manager(running).await;
}

#[tokio::test]
async fn save_config_sends_timings_after_a_successful_save() {
    let temp = tempdir().unwrap();
    let state = state_in(temp.path());
    state.set_label("Unsaved");
    let running = start_manager(true).await;

    assert!(save_config(&state, &running.manager).await);

    assert_eq!(
        running.commands.lock().unwrap().as_slice(),
        ["TIMING:25,250,450"]
    );
    assert!(!state.is_dirty());

    stop_manager(running).await;
}

#[tokio::test]
async fn failed_config_save_does_not_sync_timings() {
    let temp = tempdir().unwrap();
    let blocked_parent = temp.path().join("blocked");
    std::fs::write(&blocked_parent, "not a directory").unwrap();
    let store = ConfigStore::at(blocked_parent.join("OctoPad"));
    let state = Arc::new(AppState::new(store));
    state.set_label("Unsaved");
    let running = start_manager(true).await;

    assert!(!save_config(&state, &running.manager).await);
    assert!(running.commands.lock().unwrap().is_empty());
    assert!(state.is_dirty());

    stop_manager(running).await;
}
