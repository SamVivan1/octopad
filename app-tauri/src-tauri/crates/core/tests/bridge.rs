use std::sync::{Arc, Mutex};

use octopad_core::bridge::{connect_device_events, run_import_from_legacy_dir};
use octopad_core::config::{default_config, ConfigStore};
use octopad_core::device::{DeviceEvent, TransportKind};
use octopad_core::state::{AppEvent, AppState};
use serde_json::Value;
use tempfile::tempdir;
use tokio::sync::mpsc;

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
}
