//! Bluetooth LE transport with the Python reconnect loop.
//!
//! Port of `linux-app/ble_bridge.py::BleBridge` (whose reconnect/notify
//! semantics match the Windows `ble_bridge.py`): scan for a pad by service UUID
//! with a default-name fallback, connect, **read TX once** to fetch the identity
//! (`HELLO:…`), subscribe to TX notifications, reassemble the notification
//! stream with [`protocol::split_events`] (notifications carry no line
//! delimiter and arrive fragmented), write commands to RX, and reconnect on a
//! drop with the `3s → ×2 capped 15s` backoff reset on every successful connect
//! (Review Focus Input 4).
//!
//! Everything hardware-facing is injectable — connector, connection and
//! sleeper — so the reconnect loop is unit-tested with fakes, no hardware and
//! no wall-clock delays.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use btleplug::api::{
    Central, Manager as _, Peripheral as _, ScanFilter, ValueNotification, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::backoff::Backoff;
use super::protocol::{self, AdvertisedDevice, BleError};
use super::{DeviceEvent, TransportKind};

/// Status while looking for the pad (Python
/// `_set_status(False, "Searching for BLE pad...")`).
pub const STATUS_SEARCHING: &str = "Searching for BLE pad…";

/// Status after a successful connect (Python `"Connected via Bluetooth"`).
pub const STATUS_CONNECTED: &str = "Connected via Bluetooth";

/// Status after a mid-session drop (Python
/// `"Pad disconnected — retrying…"`).
pub const STATUS_DISCONNECTED: &str = "Pad disconnected — retrying…";

/// Scan window per attempt (Python `_SCAN_TIMEOUT_S = 10.0`).
pub const SCAN_TIMEOUT_MS: u64 = 10_000;

/// A boxed `Send` future — the transports are trait objects, so every async
/// method returns one of these instead of an `impl Future`.
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Result of the minimal connect probe (`ble_spike` parity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectProbe {
    /// The pad that was found and connected.
    pub discovered: protocol::Discovered,
    /// Raw TX read, when the pad answered it (notify-only pads may reject it).
    pub tx_read: Option<String>,
}

/// A live BLE link. Implemented for the real `btleplug` connection and for test
/// doubles, so the reconnect loop can be driven without hardware.
pub trait BleConnection: Send {
    /// The device address, in the platform's display form.
    fn address(&self) -> String;
    /// `true` when the pad exposed a writable RX characteristic.
    fn can_write(&self) -> bool;
    /// Read the TX characteristic once (the early `HELLO:` identity line).
    fn read_tx(&mut self) -> BoxFuture<'_, Result<Vec<u8>, BleError>>;
    /// Enable notifications on the TX characteristic.
    fn start_notify(&mut self) -> BoxFuture<'_, Result<(), BleError>>;
    /// The next notification payload; `Ok(None)` means "nothing yet", `Err`
    /// means the link dropped.
    fn read_notify(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, BleError>>;
    /// Write one command line to the RX characteristic (terminator added by the
    /// implementation).
    fn write_line(&mut self, line: &str) -> BoxFuture<'_, Result<(), BleError>>;
    /// Release the connection. Idempotent; called on every disconnect path.
    fn close(&mut self) -> BoxFuture<'_, ()>;
}

/// A fresh connection attempt: `Ok(None)` = no pad heard in this scan window.
pub type BleConnector = Box<
    dyn Fn() -> BoxFuture<'static, Result<Option<Box<dyn BleConnection>>, BleError>> + Send + Sync,
>;

/// Injected delay mechanism: production awaits `tokio::time::sleep`, tests
/// record the requested delay and return immediately.
pub trait Sleeper: Send + Sync {
    /// Wait for `duration`.
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()>;
}

/// Production [`Sleeper`]: an async sleep on the transport task.
pub struct TokioSleeper;

impl Sleeper for TokioSleeper {
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// One queued command for the RX characteristic.
struct Command {
    line: String,
    log: bool,
}

/// Thread-safe flags shared with every [`BleCommandHandle`].
struct SharedState {
    connected: AtomicBool,
    can_write: AtomicBool,
}

/// Thread-safe command handle, obtained from [`BleTransport::command_handle`]
/// before spawning. Mirrors Python `send_command`/`self._connected`.
pub struct BleCommandHandle {
    state: Arc<SharedState>,
    commands: mpsc::UnboundedSender<Command>,
}

impl BleCommandHandle {
    /// `true` while a pad is connected (Python `self._connected`).
    pub fn is_connected(&self) -> bool {
        self.state.connected.load(Ordering::SeqCst)
    }

    /// Queue a command for the RX characteristic (Python `send_command`).
    /// Returns `false` when no pad is connected or it exposed no RX
    /// characteristic. With `log = true` the transport emits a
    /// `[TX-BLE] …` line once the write is attempted.
    pub fn send_command(&self, command: &str, log: bool) -> bool {
        if !self.is_connected() || !self.state.can_write.load(Ordering::SeqCst) {
            return false;
        }
        self.commands
            .send(Command {
                line: command.trim().to_string(),
                log,
            })
            .is_ok()
    }
}

/// Result of one wait in a session: a notification, a queued command, or
/// shutdown. A plain enum keeps the `select!` borrow of the connection local.
enum Step {
    Notify(Result<Option<Vec<u8>>, BleError>),
    Command(Option<Command>),
    Shutdown,
}

/// Why a session ended.
enum Session {
    /// Shutdown was requested: exit the loop.
    Shutdown,
    /// The link dropped: back off and reconnect.
    Disconnected,
}

/// BLE transport. Build one with [`BleTransport::new`], override the injectable
/// pieces in tests, then hand it to [`BleTransport::spawn`].
pub struct BleTransport {
    connector: BleConnector,
    sleeper: Box<dyn Sleeper>,
    timing_command: Option<String>,
    commands_tx: mpsc::UnboundedSender<Command>,
    commands_rx: Option<mpsc::UnboundedReceiver<Command>>,
    state: Arc<SharedState>,
}

impl Default for BleTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl BleTransport {
    /// Production transport: the real `btleplug` connector, real sleeps, and
    /// the default `TIMING:` command pushed after every connect (Python
    /// `sync_timings`, with `log = false`).
    pub fn new() -> Self {
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        Self {
            connector: Box::new(|| Box::pin(connect_octopad())),
            sleeper: Box::new(TokioSleeper),
            timing_command: crate::config::timing_command(&crate::config::default_config()),
            commands_tx,
            commands_rx: Some(commands_rx),
            state: Arc::new(SharedState {
                connected: AtomicBool::new(false),
                can_write: AtomicBool::new(false),
            }),
        }
    }

    /// Replace the connection attempt (tests).
    pub fn with_connector(mut self, connector: BleConnector) -> Self {
        self.connector = connector;
        self
    }

    /// Replace the delay mechanism (tests: zero-delay recorder).
    pub fn with_sleeper(mut self, sleeper: impl Sleeper + 'static) -> Self {
        self.sleeper = Box::new(sleeper);
        self
    }

    /// Override the `TIMING:…` command pushed right after connecting
    /// (Python `sync_timings`); `None` disables the push.
    pub fn with_timing_command(mut self, command: Option<String>) -> Self {
        self.timing_command = command;
        self
    }

    /// A handle for sending commands and querying connection state. Take it
    /// before [`Self::spawn`] consumes the transport.
    pub fn command_handle(&self) -> BleCommandHandle {
        BleCommandHandle {
            state: self.state.clone(),
            commands: self.commands_tx.clone(),
        }
    }

    /// Run the reconnect loop. The loop exits when `shutdown` flips to `true`
    /// (checked before scanning, on every notify wait and before every retry
    /// sleep).
    pub fn spawn(
        mut self,
        tx: mpsc::Sender<DeviceEvent>,
        shutdown: watch::Receiver<bool>,
    ) -> JoinHandle<()> {
        let commands = self
            .commands_rx
            .take()
            .expect("a BleTransport can only be spawned once");
        let state = self.state.clone();
        tokio::spawn(async move {
            self.reconnect_loop(tx, shutdown, commands).await;
            // Never leave a stale "connected" flag behind: the UI and the
            // manager read it after shutdown.
            state.connected.store(false, Ordering::SeqCst);
            state.can_write.store(false, Ordering::SeqCst);
        })
    }

    /// The reconnect loop (port of Python `_main`/`_backoff`).
    async fn reconnect_loop(
        &self,
        tx: mpsc::Sender<DeviceEvent>,
        mut shutdown: watch::Receiver<bool>,
        mut commands: mpsc::UnboundedReceiver<Command>,
    ) {
        let mut backoff = Backoff::ble();
        let mut pending = String::new();
        let mut device_name: Option<String> = None;

        while !*shutdown.borrow() {
            send(&tx, searching_event()).await;

            let attempt = (self.connector)().await;
            let session = match attempt {
                Ok(Some(connection)) => {
                    // A successful connect restarts the ladder (Python
                    // `self._reconnect_delay = _RECONNECT_S`).
                    backoff.reset();
                    self.session(
                        &tx,
                        &mut shutdown,
                        connection,
                        &mut commands,
                        &mut pending,
                        &mut device_name,
                    )
                    .await
                }
                Ok(None) => Session::Disconnected,
                Err(err) => {
                    // Structured error, then back off — never a tight retry loop.
                    send(
                        &tx,
                        DeviceEvent::Error {
                            message: err.to_string(),
                        },
                    )
                    .await;
                    Session::Disconnected
                }
            };

            if matches!(session, Session::Shutdown) || *shutdown.borrow() {
                return;
            }
            let delay = backoff.next_delay();
            self.sleeper.sleep(delay).await;
        }
    }

    /// One connected session: read TX once, push timings, then pump
    /// notifications and queued commands until the link drops.
    async fn session(
        &self,
        tx: &mpsc::Sender<DeviceEvent>,
        shutdown: &mut watch::Receiver<bool>,
        mut connection: Box<dyn BleConnection>,
        commands: &mut mpsc::UnboundedReceiver<Command>,
        pending: &mut String,
        device_name: &mut Option<String>,
    ) -> Session {
        let address = connection.address();
        self.state
            .can_write
            .store(connection.can_write(), Ordering::SeqCst);
        self.state.connected.store(true, Ordering::SeqCst);
        send(tx, connected_event(&address)).await;

        // Read TX once: the pad answers `HELLO:` there. Re-read on every
        // connect (Review Focus Input 4); a failing read is not fatal
        // (notify-only pads reject plain reads).
        match connection.read_tx().await {
            Ok(bytes) if !bytes.is_empty() => {
                pending.push_str(&String::from_utf8_lossy(&bytes));
                for line in protocol::split_events(pending) {
                    feed_line(tx, &line, device_name).await;
                }
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(error = %err, "BLE TX read failed (notify-only pads reject reads)")
            }
        }

        // Push the configured timings right after connect, without a log line
        // (Python `sync_timings` → `_write(..., log=False)`).
        if let Some(command) = &self.timing_command {
            if let Err(err) = connection.write_line(command).await {
                tracing::debug!(error = %err, "BLE timing push failed");
            }
        }

        if let Err(err) = connection.start_notify().await {
            tracing::warn!(error = %err, "BLE subscribe failed; reconnecting");
            connection.close().await;
            self.clear_connected();
            send(tx, disconnected_event(&address)).await;
            return Session::Disconnected;
        }

        let outcome = loop {
            if *shutdown.borrow() {
                break Session::Shutdown;
            }

            let step = tokio::select! {
                result = connection.read_notify() => Step::Notify(result),
                command = commands.recv() => Step::Command(command),
                // A flipped flag (or a dropped sender) ends the loop.
                changed = shutdown.changed() => {
                    let _ = changed;
                    Step::Shutdown
                }
            };

            match step {
                Step::Shutdown => break Session::Shutdown,
                // Unreachable: the transport itself keeps a sender alive, so
                // the channel never closes while the loop runs.
                Step::Command(None) => {}
                Step::Command(Some(command)) => match connection.write_line(&command.line).await {
                    Ok(()) => {
                        if command.log {
                            send(
                                tx,
                                DeviceEvent::Log {
                                    line: format!("[TX-BLE] {}", command.line),
                                },
                            )
                            .await;
                        }
                    }
                    // Python logs the failure and keeps the link; a truly dead
                    // link surfaces through the notify stream.
                    Err(err) => tracing::debug!(error = %err, "BLE write failed"),
                },
                Step::Notify(Ok(None)) => {}
                Step::Notify(Ok(Some(chunk))) => {
                    pending.push_str(&String::from_utf8_lossy(&chunk));
                    for line in protocol::split_events(pending) {
                        feed_line(tx, &line, device_name).await;
                    }
                }
                Step::Notify(Err(err)) => {
                    tracing::debug!(error = %err, "BLE link dropped; reconnecting");
                    break Session::Disconnected;
                }
            }
        };

        connection.close().await;
        // Clear the flags before announcing the drop, so a `send_command`
        // racing the disconnect is refused.
        self.clear_connected();
        if matches!(outcome, Session::Disconnected) {
            send(tx, disconnected_event(&address)).await;
        }
        outcome
    }

    /// Drop the shared connection flags.
    fn clear_connected(&self) {
        self.state.connected.store(false, Ordering::SeqCst);
        self.state.can_write.store(false, Ordering::SeqCst);
    }
}

/// Push one received line through the Python `_feed_line` order: log it, then
/// try HELLO (which short-circuits), then the button event pattern. Mirrors
/// `serial_transport::feed_line` (private to the blocking loop there).
async fn feed_line(tx: &mpsc::Sender<DeviceEvent>, line: &str, device_name: &mut Option<String>) {
    send(
        tx,
        DeviceEvent::Log {
            line: line.to_string(),
        },
    )
    .await;

    if let Some(hello) = protocol::parse_hello(line) {
        *device_name = Some(hello.name.clone());
        send(
            tx,
            DeviceEvent::HelloReceived {
                name: Some(hello.name),
                version: hello.version,
            },
        )
        .await;
        return;
    }

    if let Some(event) = protocol::parse_event_line(line) {
        send(tx, event).await;
    }
}

fn searching_event() -> DeviceEvent {
    DeviceEvent::StatusChanged {
        transport: TransportKind::Ble,
        connected: false,
        port: None,
        message: Some(STATUS_SEARCHING.to_string()),
    }
}

fn connected_event(address: &str) -> DeviceEvent {
    DeviceEvent::StatusChanged {
        transport: TransportKind::Ble,
        connected: true,
        port: Some(address.to_string()),
        message: Some(STATUS_CONNECTED.to_string()),
    }
}

fn disconnected_event(address: &str) -> DeviceEvent {
    DeviceEvent::StatusChanged {
        transport: TransportKind::Ble,
        connected: false,
        port: Some(address.to_string()),
        message: Some(STATUS_DISCONNECTED.to_string()),
    }
}

/// Non-panicking send: a dropped receiver simply ends the event stream.
async fn send(tx: &mpsc::Sender<DeviceEvent>, event: DeviceEvent) {
    let _ = tx.send(event).await;
}

// ---------------------------------------------------------------------------
// Real `btleplug` implementation (not exercised by unit tests)
// ---------------------------------------------------------------------------

/// Production connector: one scan-and-connect attempt.
async fn connect_octopad() -> Result<Option<Box<dyn BleConnection>>, BleError> {
    let adapter = first_adapter().await?;
    let Some((_discovered, peripheral)) = find_octopad(&adapter, SCAN_TIMEOUT_MS).await? else {
        return Ok(None);
    };

    // (`_discovered` is unused here: the session learns the identity from the
    // TX read, and the address comes from the live connection.)

    peripheral
        .connect()
        .await
        .map_err(|err| protocol::classify_ble_failure("connect", &err.to_string()))?;

    match setup_connection(&peripheral).await {
        Ok(connection) => Ok(Some(Box::new(connection))),
        Err(err) => {
            // Do not leak a half-open link on a failed setup.
            if let Err(disconnect) = peripheral.disconnect().await {
                tracing::warn!(error = %disconnect, "disconnect after setup failure failed");
            }
            Err(err)
        }
    }
}

/// Discover OctoPad devices: scan for `timeout_ms`, then classify everything
/// heard (M0 spike API, kept for the `ble_spike` shim). Structured
/// [`BleError`] on failure — never a panic.
pub async fn discover_octopad(timeout_ms: u64) -> Result<Vec<protocol::Discovered>, BleError> {
    let adapter = first_adapter().await?;
    let scanned = scan(&adapter, ScanFilter::default(), timeout_ms).await?;
    Ok(scanned
        .iter()
        .filter_map(|(adv, _)| protocol::classify_device(adv))
        .collect())
}

/// Minimal connect probe (M0 spike API, kept for the `ble_spike` shim): find
/// the first OctoPad from a fresh scan, connect, read TX once, disconnect.
///
/// Returns `Ok(None)` when no adapter or no matching device is present.
pub async fn connect_and_read_tx(timeout_ms: u64) -> Result<Option<ConnectProbe>, BleError> {
    let adapter = first_adapter().await?;
    let Some((discovered, peripheral)) = find_octopad(&adapter, timeout_ms).await? else {
        return Ok(None);
    };

    peripheral
        .connect()
        .await
        .map_err(|err| protocol::classify_ble_failure("connect", &err.to_string()))?;

    let tx_read = match setup_connection(&peripheral).await {
        Ok(mut connection) => match connection.read_tx().await {
            Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
            Err(err) => {
                tracing::warn!(error = %err, "BLE TX read failed (notify-only pads reject reads)");
                None
            }
        },
        Err(err) => {
            tracing::warn!(error = %err, "BLE setup failed");
            None
        }
    };

    // Disconnect cleanly; a failed disconnect must not hide the probe result.
    if let Err(err) = peripheral.disconnect().await {
        tracing::warn!(error = %err, "disconnect failed");
    }

    Ok(Some(ConnectProbe {
        discovered,
        tx_read,
    }))
}

/// Find the first OctoPad: a service-UUID-filtered scan first (the strong
/// signal, works for renamed pads), then a plain scan for the default-name
/// fallback (some adapters do not surface advertised service UUIDs — mirrors
/// both Python bridges).
async fn find_octopad(
    adapter: &Adapter,
    timeout_ms: u64,
) -> Result<Option<(protocol::Discovered, Peripheral)>, BleError> {
    let filtered = scan(
        adapter,
        ScanFilter {
            services: service_uuids(),
        },
        timeout_ms,
    )
    .await?;
    if let Some((adv, peripheral)) = filtered
        .into_iter()
        .find(|(adv, _)| protocol::classify_device(adv).is_some())
    {
        let discovered = protocol::classify_device(&adv).expect("just matched");
        return Ok(Some((discovered, peripheral)));
    }

    let plain = scan(adapter, ScanFilter::default(), timeout_ms).await?;
    Ok(plain.into_iter().find_map(|(adv, peripheral)| {
        protocol::classify_device(&adv).map(|found| (found, peripheral))
    }))
}

/// Scan with `filter` for `timeout_ms`, collecting everything advertised paired
/// with its periphal. Unreadable entries are skipped; the scan is always
/// stopped before returning.
async fn scan(
    adapter: &Adapter,
    filter: ScanFilter,
    timeout_ms: u64,
) -> Result<Vec<(AdvertisedDevice, Peripheral)>, BleError> {
    adapter
        .start_scan(filter)
        .await
        .map_err(|err| protocol::classify_ble_failure("start_scan", &err.to_string()))?;
    tokio::time::sleep(Duration::from_millis(timeout_ms)).await;

    let mut found = Vec::new();
    for peripheral in adapter
        .peripherals()
        .await
        .map_err(|err| protocol::classify_ble_failure("peripherals", &err.to_string()))?
    {
        let address = peripheral.address().to_string();
        let Some(properties) = peripheral.properties().await.ok().flatten() else {
            continue;
        };
        found.push((
            AdvertisedDevice {
                name: properties.local_name.clone(),
                address,
                service_uuids: properties
                    .services
                    .iter()
                    .map(|uuid| uuid.to_string())
                    .collect(),
            },
            peripheral,
        ));
    }

    if let Err(err) = adapter.stop_scan().await {
        tracing::warn!(error = %err, "stop_scan failed");
    }
    Ok(found)
}

/// The primary and legacy NUS service UUIDs, for the filtered scan.
fn service_uuids() -> Vec<Uuid> {
    [
        crate::proto::SERVICE_UUID,
        crate::proto::LEGACY_SERVICE_UUID,
    ]
    .iter()
    .filter_map(|uuid| Uuid::parse_str(uuid).ok())
    .collect()
}

async fn first_adapter() -> Result<Adapter, BleError> {
    let manager = Manager::new()
        .await
        .map_err(|err| BleError::Other(format!("Bluetooth manager unavailable: {err}")))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|err| BleError::Other(format!("listing adapters failed: {err}")))?;
    adapters.into_iter().next().ok_or(BleError::NoAdapter)
}

/// Discover services and wire up a [`BleConnection`] over an already connected
/// peripheral.
async fn setup_connection(peripheral: &Peripheral) -> Result<RealConnection, BleError> {
    peripheral
        .discover_services()
        .await
        .map_err(|err| protocol::classify_ble_failure("discover_services", &err.to_string()))?;

    let characteristics = peripheral.characteristics();
    let tx = characteristics
        .iter()
        .find(|char| protocol::is_tx_characteristic(&char.uuid.to_string()))
        .cloned()
        .ok_or_else(|| BleError::Other("TX characteristic not found on device".to_string()))?;
    let rx = characteristics
        .iter()
        .find(|char| protocol::is_rx_characteristic(&char.uuid.to_string()))
        .cloned();

    Ok(RealConnection {
        peripheral: peripheral.clone(),
        tx,
        rx,
        // Subscribed lazily by `start_notify` (the M0 probe reads TX without
        // ever subscribing).
        notifications: None,
    })
}

/// A real `btleplug` peripheral adapted to [`BleConnection`].
struct RealConnection {
    peripheral: Peripheral,
    /// TX characteristic (device → host): read once, then subscribe.
    tx: btleplug::api::Characteristic,
    /// RX characteristic (host → device), when the pad exposes one.
    rx: Option<btleplug::api::Characteristic>,
    /// Notification stream, opened by [`BleConnection::start_notify`].
    notifications: Option<std::pin::Pin<Box<dyn futures::Stream<Item = ValueNotification> + Send>>>,
}

impl BleConnection for RealConnection {
    fn address(&self) -> String {
        self.peripheral.address().to_string()
    }

    fn can_write(&self) -> bool {
        self.rx.is_some()
    }

    fn read_tx(&mut self) -> BoxFuture<'_, Result<Vec<u8>, BleError>> {
        Box::pin(async move {
            self.peripheral
                .read(&self.tx)
                .await
                .map_err(|err| protocol::classify_ble_failure("read TX", &err.to_string()))
        })
    }

    fn start_notify(&mut self) -> BoxFuture<'_, Result<(), BleError>> {
        // Already subscribed in `setup_connection` (the probe may have consumed
        // the stream); a second subscribe is harmless and keeps the trait
        // contract honest.
        Box::pin(async move {
            self.peripheral
                .subscribe(&self.tx)
                .await
                .map_err(|err| protocol::classify_ble_failure("subscribe TX", &err.to_string()))?;
            let stream =
                self.peripheral.notifications().await.map_err(|err| {
                    protocol::classify_ble_failure("notifications", &err.to_string())
                })?;
            self.notifications = Some(stream);
            Ok(())
        })
    }

    fn read_notify(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, BleError>> {
        Box::pin(async move {
            let Some(notifications) = self.notifications.as_mut() else {
                return Err(BleError::Other("notifications not started".to_string()));
            };
            match notifications.next().await {
                // Only TX is subscribed; anything else is not a pad event.
                Some(notification) if notification.uuid == self.tx.uuid => {
                    Ok(Some(notification.value))
                }
                Some(_) => Ok(None),
                // The stream ends when the link is gone.
                None => Err(BleError::Other("notification stream ended".to_string())),
            }
        })
    }

    fn write_line(&mut self, line: &str) -> BoxFuture<'_, Result<(), BleError>> {
        // Own the payload before the async block: the returned future borrows
        // only `self`.
        let mut payload = line.trim().as_bytes().to_vec();
        payload.push(b'\n'); // Python: `command.strip() + "\n"`
        Box::pin(async move {
            let rx = self.rx.clone().ok_or_else(|| {
                BleError::Other("RX characteristic not found on device".to_string())
            })?;
            self.peripheral
                .write(&rx, &payload, WriteType::WithoutResponse)
                .await
                .map_err(|err| protocol::classify_ble_failure("write RX", &err.to_string()))
        })
    }

    fn close(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            if let Err(err) = self.peripheral.disconnect().await {
                tracing::warn!(error = %err, "BLE disconnect failed");
            }
        })
    }
}
