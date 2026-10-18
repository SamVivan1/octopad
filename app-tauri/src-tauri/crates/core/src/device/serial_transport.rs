//! USB-serial transport with the Python reconnect loop.
//!
//! Port of `linux-app/serial_listener.py::SerialDaemonListener` (identical to
//! the Windows listener): continuously scan for candidate ports, probe each for
//! an OctoPad HELLO, connect, push the configured timings, read lines and feed
//! them through the HELLO/event parsers, and reconnect when the USB cable is
//! pulled (Review Focus Input 3).
//!
//! The loop runs on a blocking thread (`tokio::task::spawn_blocking`) because
//! serial reads are blocking; it stops cooperatively when the `watch` shutdown
//! flag flips. Everything hardware-facing is injectable — port lister, prober,
//! opener and sleeper — so the reconnect loop is unit-tested with fakes, no
//! hardware and no wall-clock delays.

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serialport::SerialPort;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::protocol::{self, Hello, SerialError};
use super::{DeviceEvent, TransportKind};

/// Backoff when there is no candidate port, or opening one failed (Python:
/// `time.sleep(2)` in `_run_loop`).
pub const NO_PORT_RETRY: Duration = Duration::from_secs(2);

/// Backoff after a mid-session read error (USB unplug) — Python sleeps 1s
/// there so a re-appearing pad is picked up quickly.
pub const READ_ERROR_RETRY: Duration = Duration::from_secs(1);

/// Probe budget for a single candidate (Python `PROBE_TIMEOUT_S = 2.0`).
pub const PROBE_TIMEOUT_MS: u64 = 2_000;

/// Read slice for an open session (Python `serial.Serial(..., timeout=1)`
/// equivalent; a shorter slice keeps shutdown responsive).
const READ_TIMEOUT: Duration = Duration::from_millis(300);

/// A live, line-oriented serial link. Implemented for real ports and for test
/// doubles, so the reconnect loop can be driven without hardware.
pub trait SerialConnection: Send {
    /// Write one command line (terminator added by the implementation).
    fn write_line(&mut self, line: &str) -> Result<(), SerialError>;
    /// Read the next complete line; `Ok(None)` means "no line yet" (read slice
    /// timed out), `Err` means the link failed.
    fn read_line(&mut self) -> Result<Option<String>, SerialError>;
    /// Release the port. Idempotent; called on every disconnect path.
    fn close(&mut self);
}

/// Injected delay mechanism: production sleeps on the calling thread, tests
/// record the requested delay and return immediately.
pub trait Sleeper: Send + Sync {
    /// Wait for `duration`.
    fn sleep(&self, duration: Duration);
}

/// Production [`Sleeper`]: blocks the transport's blocking-pool thread.
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// Enumerates the candidate ports to scan.
pub type PortLister = Box<dyn Fn() -> Vec<String> + Send + Sync>;
/// Probes one candidate port for an OctoPad HELLO.
pub type PortProber = Box<dyn Fn(&str) -> Result<Option<Hello>, SerialError> + Send + Sync>;
/// Opens a candidate port for the session.
pub type PortOpener =
    Box<dyn Fn(&str) -> Result<Box<dyn SerialConnection>, SerialError> + Send + Sync>;

/// USB serial transport. Build one with [`SerialTransport::new`], override the
/// injectable pieces in tests, then hand it to [`SerialTransport::spawn`].
pub struct SerialTransport {
    lister: PortLister,
    prober: PortProber,
    opener: PortOpener,
    sleeper: Box<dyn Sleeper>,
    timing_command: Option<String>,
    commands_tx: mpsc::UnboundedSender<Command>,
    commands_rx: Option<mpsc::UnboundedReceiver<Command>>,
    connected: Arc<AtomicBool>,
}

struct Command {
    line: String,
    log: bool,
}

/// Thread-safe command handle for the live serial transport.
#[derive(Clone)]
pub struct SerialCommandHandle {
    connected: Arc<AtomicBool>,
    commands: mpsc::UnboundedSender<Command>,
}

impl SerialCommandHandle {
    /// Queue a command for the connected serial session.
    pub fn send_command(&self, command: &str) -> bool {
        self.connected.load(Ordering::SeqCst)
            && self
                .commands
                .send(Command {
                    line: command.trim().to_string(),
                    log: false,
                })
                .is_ok()
    }
}

impl Default for SerialTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl SerialTransport {
    /// Production transport: system port enumeration, the M0 HELLO probe, a
    /// real serial opener, real sleeps, and the default `TIMING:` command.
    pub fn new() -> Self {
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        Self {
            lister: Box::new(|| protocol::list_candidate_ports().unwrap_or_default()),
            prober: Box::new(|port: &str| protocol::probe_hello(port, PROBE_TIMEOUT_MS)),
            opener: Box::new(open_port),
            sleeper: Box::new(ThreadSleeper),
            timing_command: crate::config::timing_command(&crate::config::default_config()),
            commands_tx,
            commands_rx: Some(commands_rx),
            connected: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A handle for queuing commands and checking connection state.
    pub fn command_handle(&self) -> SerialCommandHandle {
        SerialCommandHandle {
            connected: self.connected.clone(),
            commands: self.commands_tx.clone(),
        }
    }

    /// Replace the candidate-port enumeration (tests).
    pub fn with_lister(mut self, lister: impl Fn() -> Vec<String> + Send + Sync + 'static) -> Self {
        self.lister = Box::new(lister);
        self
    }

    /// Replace the HELLO probe (tests).
    pub fn with_prober(
        mut self,
        prober: impl Fn(&str) -> Result<Option<Hello>, SerialError> + Send + Sync + 'static,
    ) -> Self {
        self.prober = Box::new(prober);
        self
    }

    /// Replace the session opener (tests).
    pub fn with_opener(
        mut self,
        opener: impl Fn(&str) -> Result<Box<dyn SerialConnection>, SerialError> + Send + Sync + 'static,
    ) -> Self {
        self.opener = Box::new(opener);
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

    /// Run the reconnect loop on a blocking-pool task. The loop exits when
    /// `shutdown` flips to `true` (checked between candidates, between reads
    /// and before every retry sleep).
    pub fn spawn(
        mut self,
        tx: mpsc::Sender<DeviceEvent>,
        shutdown: watch::Receiver<bool>,
    ) -> JoinHandle<()> {
        let commands = self
            .commands_rx
            .take()
            .expect("a SerialTransport can only be spawned once");
        let connected = self.connected.clone();
        tokio::task::spawn_blocking(move || {
            self.run(tx, shutdown, commands);
            connected.store(false, Ordering::SeqCst);
        })
    }

    /// The blocking reconnect loop (port of `_run_loop`).
    fn run(
        &self,
        tx: mpsc::Sender<DeviceEvent>,
        shutdown: watch::Receiver<bool>,
        mut commands: mpsc::UnboundedReceiver<Command>,
    ) {
        let mut last_port: Option<String> = None;
        let mut device_name: Option<String> = None;

        while !*shutdown.borrow() {
            let candidates = (self.lister)();
            let Some(port) = self.pick_port(&candidates, &last_port, device_name.as_deref()) else {
                // No candidate: back off 2s and rescan (never busy-spin).
                if *shutdown.borrow() {
                    return;
                }
                self.sleeper.sleep(NO_PORT_RETRY);
                continue;
            };

            let mut connection = match (self.opener)(&port) {
                Ok(connection) => connection,
                Err(err) => {
                    tracing::debug!(port, error = %err, "serial open failed; rescanning");
                    if *shutdown.borrow() {
                        return;
                    }
                    self.sleeper.sleep(NO_PORT_RETRY);
                    continue;
                }
            };

            last_port = Some(port.clone());
            self.connected.store(true, Ordering::SeqCst);
            send(
                &tx,
                DeviceEvent::StatusChanged {
                    transport: TransportKind::Serial,
                    connected: true,
                    port: Some(port.clone()),
                    message: None,
                },
            );

            // Learn/confirm the identity, then push the configured timings
            // (Python writes WHO and calls sync_timings right after connect).
            let _ = connection.write_line(protocol::WHO_COMMAND);
            if let Some(command) = &self.timing_command {
                let _ = connection.write_line(command);
            }

            // Read loop: stay on this connection until it fails.
            loop {
                if *shutdown.borrow() {
                    connection.close();
                    self.connected.store(false, Ordering::SeqCst);
                    return;
                }
                while let Ok(command) = commands.try_recv() {
                    if connection.write_line(&command.line).is_ok() && command.log {
                        send(
                            &tx,
                            DeviceEvent::Log {
                                line: format!("[TX] {}", command.line),
                            },
                        );
                    }
                }
                match connection.read_line() {
                    Ok(Some(line)) => {
                        if line.is_empty() {
                            continue;
                        }
                        feed_line(&tx, &line, &mut device_name);
                    }
                    // Read slice timed out with no data: keep listening.
                    Ok(None) => {}
                    Err(err) => {
                        tracing::debug!(port, error = %err, "serial read failed; reconnecting");
                        connection.close();
                        self.connected.store(false, Ordering::SeqCst);
                        send(
                            &tx,
                            DeviceEvent::StatusChanged {
                                transport: TransportKind::Serial,
                                connected: false,
                                port: Some(port.clone()),
                                message: None,
                            },
                        );
                        break;
                    }
                }
            }

            // Mid-session failure: 1s before rescanning.
            if *shutdown.borrow() {
                return;
            }
            self.sleeper.sleep(READ_ERROR_RETRY);
        }
    }

    /// Choose the port to open, mirroring Python `_pick_port`: prefer the last
    /// connected port once its identity is known, otherwise probe candidates
    /// and accept the only candidate even when it does not answer (older
    /// firmware without the handshake keeps working in single-device setups).
    fn pick_port(
        &self,
        candidates: &[String],
        last_port: &Option<String>,
        device_name: Option<&str>,
    ) -> Option<String> {
        if candidates.is_empty() {
            return None;
        }
        if let Some(last) = last_port {
            if device_name.is_some() && candidates.contains(last) {
                return Some(last.clone());
            }
        }
        for port in candidates {
            if let Ok(Some(_)) = (self.prober)(port) {
                return Some(port.clone());
            }
        }
        (candidates.len() == 1).then(|| candidates[0].clone())
    }
}

/// Push one received line through the Python `_run_loop` order: log it, then
/// try HELLO (which short-circuits), then the button event pattern.
fn feed_line(tx: &mpsc::Sender<DeviceEvent>, line: &str, device_name: &mut Option<String>) {
    send(
        tx,
        DeviceEvent::Log {
            line: line.to_string(),
        },
    );

    if let Some(hello) = protocol::parse_hello(line) {
        *device_name = Some(hello.name.clone());
        send(
            tx,
            DeviceEvent::HelloReceived {
                name: Some(hello.name),
                version: hello.version,
            },
        );
        return;
    }

    if let Some(event) = protocol::parse_event_line(line) {
        send(tx, event);
    }
}

/// Non-panicking send: a dropped receiver simply ends the event stream.
fn send(tx: &mpsc::Sender<DeviceEvent>, event: DeviceEvent) {
    let _ = tx.blocking_send(event);
}

/// Open a real serial port for the session.
fn open_port(port: &str) -> Result<Box<dyn SerialConnection>, SerialError> {
    let handle = serialport::new(port, protocol::BAUD_RATE)
        .timeout(READ_TIMEOUT)
        .open()
        .map_err(|err| protocol::classify_port_failure(&format!("open {port}"), &err))?;
    Ok(Box::new(RealConnection {
        port: Some(handle),
        pending: Vec::new(),
    }))
}

/// A real `serialport` handle adapted to [`SerialConnection`].
struct RealConnection {
    /// `None` once [`SerialConnection::close`] released the port.
    port: Option<Box<dyn SerialPort>>,
    /// Bytes of an incomplete line, carried across reads.
    pending: Vec<u8>,
}

impl RealConnection {
    /// The open handle, or a structured error once `close` released it.
    fn link(&mut self) -> Result<&mut Box<dyn SerialPort>, SerialError> {
        self.port
            .as_mut()
            .ok_or_else(|| SerialError::Other("serial port closed".to_string()))
    }
}

impl SerialConnection for RealConnection {
    fn write_line(&mut self, line: &str) -> Result<(), SerialError> {
        let mut payload = line.trim().as_bytes().to_vec();
        payload.push(b'\n'); // Python: `command.strip() + "\n"`
        self.link()?
            .write_all(&payload)
            .map_err(|err| classify_io("write", &err))
    }

    fn read_line(&mut self) -> Result<Option<String>, SerialError> {
        let mut chunk = [0u8; 256];
        loop {
            match self.link()?.read(&mut chunk) {
                // Defensive: never busy-spin on a non-blocking empty read.
                Ok(0) => return Ok(None),
                Ok(n) => {
                    self.pending.extend_from_slice(&chunk[..n]);
                    if let Some(line) = protocol::take_line(&mut self.pending) {
                        return Ok(Some(line));
                    }
                }
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) =>
                {
                    return Ok(None);
                }
                Err(err) => return Err(classify_io("read", &err)),
            }
        }
    }

    fn close(&mut self) {
        // Dropping the handle clears TIOCEXCL and closes the fd, releasing the
        // port for the next client *before* the backoff sleep.
        self.port = None;
    }
}

/// Classify an `io::Error` surfaced through `serialport`.
fn classify_io(op: &str, err: &io::Error) -> SerialError {
    let wrapped = serialport::Error::from(io::Error::new(err.kind(), err.to_string()));
    protocol::classify_port_failure(op, &wrapped)
}
