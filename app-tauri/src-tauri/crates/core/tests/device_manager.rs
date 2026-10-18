use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use octopad_core::config::default_config;
use octopad_core::device::manager::{DeviceManager, DeviceTransport};
use octopad_core::device::{DeviceEvent, TransportKind};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

#[derive(Clone)]
struct FakeTransport {
    connected: Arc<AtomicBool>,
    commands: Arc<Mutex<Vec<String>>>,
    events: Vec<DeviceEvent>,
}

impl FakeTransport {
    fn new(connected: bool, events: Vec<DeviceEvent>) -> Self {
        Self {
            connected: Arc::new(AtomicBool::new(connected)),
            commands: Arc::new(Mutex::new(Vec::new())),
            events,
        }
    }

    fn commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }
}

impl DeviceTransport for FakeTransport {
    fn send_command(&self, command: &str) -> bool {
        if !self.connected.load(Ordering::SeqCst) {
            return false;
        }
        self.commands.lock().unwrap().push(command.to_string());
        true
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

fn start_manager(serial: FakeTransport, ble: FakeTransport) -> RunningManager {
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

fn status(transport: TransportKind, connected: bool) -> DeviceEvent {
    DeviceEvent::StatusChanged {
        transport,
        connected,
        port: None,
        message: None,
    }
}

async fn wait_for_statuses(
    events: &mut mpsc::Receiver<DeviceEvent>,
    serial_connected: bool,
    ble_connected: bool,
) {
    let mut serial_seen = false;
    let mut ble_seen = false;
    while !serial_seen || !ble_seen {
        match events.recv().await.unwrap() {
            DeviceEvent::StatusChanged {
                transport: TransportKind::Serial,
                connected,
                ..
            } => serial_seen = connected == serial_connected,
            DeviceEvent::StatusChanged {
                transport: TransportKind::Ble,
                connected,
                ..
            } => ble_seen = connected == ble_connected,
            event => panic!("unexpected event before connection statuses: {event:?}"),
        }
    }
}

async fn wait_for_command_count(transport: &FakeTransport, count: usize) {
    for _ in 0..100 {
        if transport.commands().len() >= count {
            return;
        }
        tokio::task::yield_now().await;
    }
    assert!(transport.commands().len() >= count);
}

#[tokio::test]
async fn send_command_prefers_serial_and_logs_only_when_requested() {
    let serial = FakeTransport::new(true, vec![status(TransportKind::Serial, true)]);
    let ble = FakeTransport::new(true, vec![status(TransportKind::Ble, true)]);
    let mut running = start_manager(serial.clone(), ble.clone());

    wait_for_statuses(&mut running.events, true, true).await;
    wait_for_command_count(&serial, 1).await;
    assert!(running.manager.send_command("X", true));
    assert_eq!(serial.commands(), vec!["TIMING:25,250,450", "X"]);
    assert!(ble.commands().is_empty());
    assert_eq!(
        running.events.recv().await,
        Some(DeviceEvent::Log {
            line: "[TX] X".to_string()
        })
    );

    assert!(running.manager.send_command("Y", false));
    assert_eq!(serial.commands(), vec!["TIMING:25,250,450", "X", "Y"]);
    assert!(running.events.try_recv().is_err());
    stop_manager(running).await;
}

#[tokio::test]
async fn send_command_falls_back_to_ble_when_serial_is_disconnected() {
    let serial = FakeTransport::new(false, vec![status(TransportKind::Serial, false)]);
    let ble = FakeTransport::new(true, vec![status(TransportKind::Ble, true)]);
    let mut running = start_manager(serial.clone(), ble.clone());

    let mut observed_status = 0;
    while observed_status < 2 {
        if matches!(
            running.events.recv().await,
            Some(DeviceEvent::StatusChanged { .. })
        ) {
            observed_status += 1;
        }
    }
    assert!(running.manager.send_command("X", true));
    assert!(serial.commands().is_empty());
    assert_eq!(ble.commands(), vec!["X"]);
    assert_eq!(
        running.events.recv().await,
        Some(DeviceEvent::Log {
            line: "[TX-BLE] X".to_string()
        })
    );
    stop_manager(running).await;
}

#[tokio::test]
async fn send_command_returns_false_when_neither_transport_is_connected() {
    let serial = FakeTransport::new(false, vec![status(TransportKind::Serial, false)]);
    let ble = FakeTransport::new(false, vec![status(TransportKind::Ble, false)]);
    let running = start_manager(serial.clone(), ble.clone());

    assert!(!running.manager.send_command("X", true));
    assert!(serial.commands().is_empty());
    assert!(ble.commands().is_empty());
    stop_manager(running).await;
}

#[tokio::test]
async fn sync_timings_routes_without_logging_and_rejects_invalid_config() {
    let serial = FakeTransport::new(true, vec![status(TransportKind::Serial, true)]);
    let ble = FakeTransport::new(true, vec![status(TransportKind::Ble, true)]);
    let mut running = start_manager(serial.clone(), ble.clone());
    let mut config = default_config();

    let mut observed_status = 0;
    while observed_status < 2 {
        if matches!(
            running.events.recv().await,
            Some(DeviceEvent::StatusChanged { .. })
        ) {
            observed_status += 1;
        }
    }
    wait_for_command_count(&serial, 1).await;
    assert!(running.manager.sync_timings(&config));
    assert_eq!(
        serial.commands(),
        vec!["TIMING:25,250,450", "TIMING:25,250,450"]
    );
    assert!(ble.commands().is_empty());
    assert!(running.events.try_recv().is_err());

    config.debounce_ms = serde_json::Value::String("not-a-number".to_string());
    assert!(!running.manager.sync_timings(&config));
    assert_eq!(serial.commands().len(), 2);
    stop_manager(running).await;
}

#[tokio::test]
async fn rename_device_strips_whitespace_and_rejects_invalid_names() {
    let serial = FakeTransport::new(true, vec![status(TransportKind::Serial, true)]);
    let ble = FakeTransport::new(false, vec![status(TransportKind::Ble, false)]);
    let mut running = start_manager(serial.clone(), ble);

    wait_for_statuses(&mut running.events, true, false).await;
    wait_for_command_count(&serial, 1).await;
    assert!(!running.manager.rename_device(""));
    assert!(!running.manager.rename_device("   "));
    assert!(!running.manager.rename_device("1234567890123456789012345"));
    assert_eq!(serial.commands(), vec!["TIMING:25,250,450"]);

    assert!(running.manager.rename_device("  My Pad  "));
    assert_eq!(serial.commands(), vec!["TIMING:25,250,450", "NAME:My Pad"]);
    assert_eq!(
        running.events.recv().await,
        Some(DeviceEvent::Log {
            line: "[TX] NAME:My Pad".to_string()
        })
    );
    stop_manager(running).await;
}

#[tokio::test]
async fn merges_hello_status_and_log_events_from_both_transports_once() {
    let serial_events = vec![
        DeviceEvent::HelloReceived {
            name: Some("Serial".to_string()),
            version: "1".to_string(),
        },
        status(TransportKind::Serial, false),
        DeviceEvent::Log {
            line: "serial".to_string(),
        },
    ];
    let ble_events = vec![
        DeviceEvent::HelloReceived {
            name: Some("BLE".to_string()),
            version: "2".to_string(),
        },
        status(TransportKind::Ble, false),
        DeviceEvent::Log {
            line: "ble".to_string(),
        },
    ];
    let mut running = start_manager(
        FakeTransport::new(false, serial_events.clone()),
        FakeTransport::new(false, ble_events.clone()),
    );

    let mut observed = Vec::new();
    for _ in 0..serial_events.len() + ble_events.len() {
        observed.push(running.events.recv().await.unwrap());
    }
    for expected in serial_events.iter().chain(ble_events.iter()) {
        assert_eq!(
            observed.iter().filter(|event| *event == expected).count(),
            1
        );
    }
    assert_eq!(observed.len(), 6);
    stop_manager(running).await;
}

#[tokio::test]
async fn pushes_default_timings_after_serial_connect_status() {
    let serial = FakeTransport::new(true, vec![status(TransportKind::Serial, true)]);
    let ble = FakeTransport::new(false, Vec::new());
    let mut running = start_manager(serial.clone(), ble);

    assert_eq!(
        running.events.recv().await,
        Some(status(TransportKind::Serial, true))
    );
    wait_for_command_count(&serial, 1).await;
    assert_eq!(serial.commands(), vec!["TIMING:25,250,450"]);
    stop_manager(running).await;
}
