//! Unified command routing and event merging for serial and BLE transports.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::config::{self, Config};

use super::{DeviceEvent, TransportKind};

/// A transport the manager can spawn and route commands through.
pub trait DeviceTransport: Send + Sync {
    /// Spawn the transport's event producer.
    fn spawn(
        &self,
        tx: mpsc::Sender<DeviceEvent>,
        shutdown: watch::Receiver<bool>,
    ) -> JoinHandle<()>;

    /// Send one command. Logging is centralized by [`DeviceManager`].
    fn send_command(&self, command: &str) -> bool;
}

/// Serial-preferred device manager.
#[derive(Clone)]
pub struct DeviceManager {
    serial: Arc<dyn DeviceTransport>,
    ble: Arc<dyn DeviceTransport>,
    serial_connected: Arc<AtomicBool>,
    ble_connected: Arc<AtomicBool>,
    log_events: Arc<Mutex<Option<mpsc::UnboundedSender<DeviceEvent>>>>,
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceManager {
    /// Create a manager using the hardware transports enabled at compile time.
    pub fn new() -> Self {
        #[cfg(feature = "serial")]
        let serial: Arc<dyn DeviceTransport> = {
            let transport =
                super::serial_transport::SerialTransport::new().with_timing_command(None);
            let command_handle = transport.command_handle();
            Arc::new(SerialAdapter {
                transport: Mutex::new(Some(transport)),
                command_handle,
            })
        };
        #[cfg(not(feature = "serial"))]
        let serial: Arc<dyn DeviceTransport> = Arc::new(UnavailableTransport);

        #[cfg(feature = "ble")]
        let ble: Arc<dyn DeviceTransport> = {
            let transport = super::ble_transport::BleTransport::new();
            let command_handle = transport.command_handle();
            Arc::new(BleAdapter {
                transport: Mutex::new(Some(transport)),
                command_handle,
            })
        };
        #[cfg(not(feature = "ble"))]
        let ble: Arc<dyn DeviceTransport> = Arc::new(UnavailableTransport);

        Self::with_transports(serial, ble)
    }

    /// Create a manager with injectable transports (for tests and embedders).
    pub fn with_transports(
        serial: Arc<dyn DeviceTransport>,
        ble: Arc<dyn DeviceTransport>,
    ) -> Self {
        Self {
            serial,
            ble,
            serial_connected: Arc::new(AtomicBool::new(false)),
            ble_connected: Arc::new(AtomicBool::new(false)),
            log_events: Arc::new(Mutex::new(None)),
        }
    }

    /// Start both transports and merge their events into `tx`.
    pub fn spawn(
        &self,
        tx: mpsc::Sender<DeviceEvent>,
        shutdown: watch::Receiver<bool>,
    ) -> JoinHandle<()> {
        let manager = self.clone();
        let (serial_tx, mut serial_rx) = mpsc::channel(64);
        let (ble_tx, mut ble_rx) = mpsc::channel(64);
        let (log_tx, mut log_rx) = mpsc::unbounded_channel();
        *self.log_events.lock().unwrap() = Some(log_tx);

        self.serial_connected.store(false, Ordering::SeqCst);
        self.ble_connected.store(false, Ordering::SeqCst);
        let serial_task = self.serial.spawn(serial_tx, shutdown.clone());
        let ble_task = self.ble.spawn(ble_tx, shutdown.clone());

        tokio::spawn(async move {
            let mut shutdown = shutdown;
            let mut serial_open = true;
            let mut ble_open = true;

            while !*shutdown.borrow() {
                tokio::select! {
                    changed = shutdown.changed() => {
                        let _ = changed;
                        break;
                    }
                    event = serial_rx.recv(), if serial_open => match event {
                        Some(event) => {
                            if !forward_event(event, TransportKind::Serial, &tx, &manager).await {
                                break;
                            }
                        }
                        None => serial_open = false,
                    },
                    event = ble_rx.recv(), if ble_open => match event {
                        Some(event) => {
                            if !forward_event(event, TransportKind::Ble, &tx, &manager).await {
                                break;
                            }
                        }
                        None => ble_open = false,
                    },
                    event = log_rx.recv() => match event {
                        Some(event) => {
                            if tx.send(event).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    },
                    else => break,
                }

                if !serial_open && !ble_open {
                    break;
                }
            }

            let _ = serial_task.await;
            let _ = ble_task.await;
        })
    }

    /// Send a command over serial first, then BLE if serial is unavailable.
    pub fn send_command(&self, command: &str, log: bool) -> bool {
        let command = command.trim();
        let (transport, sent) = if self.serial_connected.load(Ordering::SeqCst) {
            if self.serial.send_command(command) {
                (Some(TransportKind::Serial), true)
            } else if self.ble_connected.load(Ordering::SeqCst) && self.ble.send_command(command) {
                (Some(TransportKind::Ble), true)
            } else {
                (None, false)
            }
        } else if self.ble_connected.load(Ordering::SeqCst) && self.ble.send_command(command) {
            (Some(TransportKind::Ble), true)
        } else {
            (None, false)
        };

        if sent && log {
            let prefix = match transport {
                Some(TransportKind::Serial) => "[TX]",
                Some(TransportKind::Ble) => "[TX-BLE]",
                None => unreachable!("a successful command has a selected transport"),
            };
            if let Some(events) = self.log_events.lock().unwrap().as_ref() {
                let _ = events.send(DeviceEvent::Log {
                    line: format!("{prefix} {command}"),
                });
            }
        }
        sent
    }

    /// Synchronize timings if they are valid, without emitting a TX log.
    pub fn sync_timings(&self, config: &Config) -> bool {
        let Some(command) = config::timing_command(config) else {
            return false;
        };
        self.send_command(&command, false)
    }

    /// Strip and validate a device name before sending its rename command.
    pub fn rename_device(&self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 24 {
            return false;
        }
        self.send_command(&format!("NAME:{name}"), true)
    }
}

async fn forward_event(
    event: DeviceEvent,
    transport: TransportKind,
    tx: &mpsc::Sender<DeviceEvent>,
    manager: &DeviceManager,
) -> bool {
    if let DeviceEvent::StatusChanged { connected, .. } = &event {
        match transport {
            TransportKind::Serial => manager.serial_connected.store(*connected, Ordering::SeqCst),
            TransportKind::Ble => manager.ble_connected.store(*connected, Ordering::SeqCst),
        }
    }
    let push_timings = matches!(
        (&event, transport),
        (
            DeviceEvent::StatusChanged {
                connected: true,
                ..
            },
            TransportKind::Serial
        )
    );
    if tx.send(event).await.is_err() {
        return false;
    }
    if push_timings {
        let _ = manager.sync_timings(&config::default_config());
    }
    true
}

#[cfg(feature = "serial")]
struct SerialAdapter {
    transport: Mutex<Option<super::serial_transport::SerialTransport>>,
    command_handle: super::serial_transport::SerialCommandHandle,
}

#[cfg(feature = "serial")]
impl DeviceTransport for SerialAdapter {
    fn spawn(
        &self,
        tx: mpsc::Sender<DeviceEvent>,
        shutdown: watch::Receiver<bool>,
    ) -> JoinHandle<()> {
        match self.transport.lock().unwrap().take() {
            Some(transport) => transport.spawn(tx, shutdown),
            None => tokio::spawn(async {}),
        }
    }

    fn send_command(&self, command: &str) -> bool {
        self.command_handle.send_command(command)
    }
}

#[cfg(feature = "ble")]
struct BleAdapter {
    transport: Mutex<Option<super::ble_transport::BleTransport>>,
    command_handle: super::ble_transport::BleCommandHandle,
}

#[cfg(feature = "ble")]
impl DeviceTransport for BleAdapter {
    fn spawn(
        &self,
        tx: mpsc::Sender<DeviceEvent>,
        shutdown: watch::Receiver<bool>,
    ) -> JoinHandle<()> {
        match self.transport.lock().unwrap().take() {
            Some(transport) => transport.spawn(tx, shutdown),
            None => tokio::spawn(async {}),
        }
    }

    fn send_command(&self, command: &str) -> bool {
        self.command_handle.send_command(command, false)
    }
}

#[cfg(any(not(feature = "serial"), not(feature = "ble")))]
struct UnavailableTransport;

#[cfg(any(not(feature = "serial"), not(feature = "ble")))]
impl DeviceTransport for UnavailableTransport {
    fn spawn(
        &self,
        _tx: mpsc::Sender<DeviceEvent>,
        mut shutdown: watch::Receiver<bool>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            if !*shutdown.borrow() {
                let _ = shutdown.changed().await;
            }
        })
    }

    fn send_command(&self, _command: &str) -> bool {
        false
    }
}
