//! Task 5: serial device transport — event-line parsing and the reconnect loop.
//!
//! The parser cases mirror `linux-app/serial_listener.py::parse_event_line`
//! (`EVENT_PATTERN`: mode `desktop|ha`, 0-based index, `single|double|hold`,
//! case-insensitive) and the HELLO cases mirror `tests/test_parsers.py`.
//!
//! The transport itself is driven through injected port-lister / prober /
//! opener / sleeper fakes, so the reconnect loop is exercised deterministically
//! without hardware and without wall-clock delays (Review Focus Input 3: the
//! port disappears mid-connection; the loop must emit disconnected, return to
//! scanning, and never panic).

#![cfg(feature = "serial")]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use octopad_core::device::protocol::{candidate_ports, parse_event_line, parse_hello};
use octopad_core::device::serial_transport::{SerialConnection, SerialTransport, Sleeper};
use octopad_core::device::{DeviceEvent, TransportKind};
use octopad_core::serial_spike::{Hello, SerialError};

// ---------------------------------------------------------------------------
// Event-line parsing (`parse_event_line` == Python `EVENT_PATTERN` semantics)
// ---------------------------------------------------------------------------

fn button(mode: &str, index: i64, trigger: &str) -> DeviceEvent {
    DeviceEvent::ButtonEvent {
        mode: mode.to_string(),
        index,
        trigger: trigger.to_string(),
    }
}

#[test]
fn parses_desktop_single_event_with_zero_based_index() {
    // Python: int(group(2)) - 1 — "Button 3" is index 2.
    assert_eq!(
        parse_event_line("[DESKTOP] Button 3 -> SINGLE"),
        Some(button("desktop", 2, "single"))
    );
}

#[test]
fn parses_ha_variants() {
    assert_eq!(
        parse_event_line("[HA] Button 1 -> HOLD"),
        Some(button("ha", 0, "hold"))
    );
    assert_eq!(
        parse_event_line("[HA] Button 8 -> DOUBLE"),
        Some(button("ha", 7, "double"))
    );
}

#[test]
fn event_line_parsing_is_case_insensitive() {
    assert_eq!(
        parse_event_line("[desktop] button 5 -> hold"),
        Some(button("desktop", 4, "hold"))
    );
    assert_eq!(
        parse_event_line("[Ha] bUtToN 1 -> DoUbLe"),
        Some(button("ha", 0, "double")),
    );
}

#[test]
fn finds_an_event_inside_surrounding_noise() {
    // Python uses `re.search`, not `fullmatch`.
    assert_eq!(
        parse_event_line("[BOOT] ok [HA] Button 2 -> DOUBLE tail"),
        Some(button("ha", 1, "double"))
    );
}

#[test]
fn ignores_noise_mode_and_partial_lines() {
    assert_eq!(parse_event_line(""), None);
    assert_eq!(parse_event_line("MODE: HOME ASSISTANT"), None);
    assert_eq!(parse_event_line("[DESKTOP] Button 3 -> CLICK"), None);
    // An out-of-range-looking index is still a valid digit run.
    assert_eq!(parse_event_line("[NOPE] Button 3 -> SINGLE"), None);
}

#[test]
fn requires_python_whitespace_between_tokens() {
    // EVENT_PATTERN uses `\s+` after `]`, after `Button` and around `->`.
    assert_eq!(parse_event_line("[DESKTOP]Button 3 -> SINGLE"), None);
    assert_eq!(parse_event_line("[DESKTOP] Button3 -> SINGLE"), None);
    assert_eq!(parse_event_line("[DESKTOP] Button 3->SINGLE"), None);
    assert_eq!(parse_event_line("[DESKTOP] Button 3 - SINGLE"), None);
}

// ---------------------------------------------------------------------------
// HELLO parsing (same cases as `tests/test_parsers.py`)
// ---------------------------------------------------------------------------

#[test]
fn parse_hello_extracts_name_and_version() {
    let hello = parse_hello("HELLO:OCTOPAD:My Pad:v2.6").expect("must parse");
    assert_eq!(hello.name, "My Pad");
    assert_eq!(hello.version, "2.6");
}

#[test]
fn parse_hello_without_version_and_with_default_name() {
    let hello = parse_hello("HELLO:OCTOPAD:My Pad").expect("must parse");
    assert_eq!(hello.version, "");
    let unnamed = parse_hello("HELLO:OCTOPAD::v2.6").expect("must parse");
    assert_eq!(unnamed.name, "OctoPad");
    assert_eq!(unnamed.version, "2.6");
}

#[test]
fn parse_hello_gates_on_the_brand_tag_case_insensitively() {
    assert!(parse_hello("HELLO:OTHER:Pad:v1").is_none());
    assert!(parse_hello("[DESKTOP] Button 3 -> SINGLE").is_none());
    let hello = parse_hello("[BOOT] hello:octopad:Pad:v1.2").expect("must parse");
    assert_eq!(hello.name, "Pad");
    assert_eq!(hello.version, "1.2");
}

// ---------------------------------------------------------------------------
// Candidate port enumeration
// ---------------------------------------------------------------------------

#[test]
fn candidate_ports_keep_only_octopad_like_ports_sorted() {
    let ports = vec![
        "/dev/ttyUSB0".to_string(),
        "/dev/ttyS0".to_string(),
        "/dev/ttyACM1".to_string(),
        "COM3".to_string(),
    ];
    let picked = candidate_ports(ports);

    #[cfg(not(target_os = "windows"))]
    assert_eq!(picked, vec!["/dev/ttyACM1", "/dev/ttyUSB0"]);

    #[cfg(target_os = "windows")]
    assert_eq!(picked, vec!["COM3"]);
}

// ---------------------------------------------------------------------------
// Transport loop with injected fakes
// ---------------------------------------------------------------------------

/// A scripted stand-in for an open serial port: replays `script` line by line
/// (so a fake opener can act out hello → button event → disconnect) and records
/// every line the transport writes.
struct ScriptedConnection {
    script: VecDeque<Result<Option<String>, SerialError>>,
    writes: Arc<Mutex<Vec<String>>>,
    closed: Arc<AtomicBool>,
}

impl SerialConnection for ScriptedConnection {
    fn write_line(&mut self, line: &str) -> Result<(), SerialError> {
        self.writes.lock().unwrap().push(line.to_string());
        Ok(())
    }

    fn read_line(&mut self) -> Result<Option<String>, SerialError> {
        self.script.pop_front().unwrap_or(Ok(None))
    }

    fn close(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

/// Zero-delay sleeper that records the requested durations (and the order).
struct RecordingSleeper {
    durations: Arc<Mutex<Vec<Duration>>>,
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) {
        self.durations.lock().unwrap().push(duration);
    }
}

/// Review Focus Input 3: one connect → HELLO → button event → read error
/// (USB unplug) cycle. The loop must emit the exact event sequence, push the
/// timing command right after connecting, emit `connected: false` on the read
/// error, and then back off 1s before rescanning.
#[tokio::test]
async fn emits_connect_hello_button_and_disconnect_sequence_then_rescans() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    let writes = Arc::new(Mutex::new(Vec::new()));
    let durations = Arc::new(Mutex::new(Vec::new()));
    let opens = Arc::new(AtomicUsize::new(0));

    let transport = SerialTransport::new()
        .with_lister(|| vec!["/dev/ttyACM0".to_string()])
        .with_prober(|_port: &str| {
            Ok(Some(Hello {
                name: "Pad".to_string(),
                version: "1.2".to_string(),
            }))
        })
        .with_opener({
            let writes = writes.clone();
            let opens = opens.clone();
            move |_port: &str| {
                // First open is the live session; every later open fails, so
                // the loop settles into the 2s "open failed" branch.
                if opens.fetch_add(1, Ordering::SeqCst) > 0 {
                    return Err(SerialError::NotFound("port vanished".to_string()));
                }
                let connection = ScriptedConnection {
                    script: VecDeque::from(vec![
                        Ok(Some("HELLO:OCTOPAD:Pad:v1.2".to_string())),
                        Ok(Some("[DESKTOP] Button 3 -> SINGLE".to_string())),
                        Err(SerialError::Other("device disappeared".to_string())),
                    ]),
                    writes: writes.clone(),
                    closed: Arc::new(AtomicBool::new(false)),
                };
                Ok(Box::new(connection) as Box<dyn SerialConnection>)
            }
        })
        .with_sleeper(RecordingSleeper {
            durations: durations.clone(),
        });

    let handle = transport.spawn(tx, shutdown_rx);

    let mut events = Vec::new();
    loop {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("the transport must not stall on wall-clock sleeps")
            .expect("the channel stays open while the transport runs");
        let disconnected = matches!(
            event,
            DeviceEvent::StatusChanged {
                connected: false,
                ..
            }
        );
        events.push(event);
        if disconnected {
            break;
        }
    }

    assert_eq!(
        events,
        vec![
            DeviceEvent::StatusChanged {
                transport: TransportKind::Serial,
                connected: true,
                port: Some("/dev/ttyACM0".to_string()),
                message: None,
            },
            DeviceEvent::Log {
                line: "HELLO:OCTOPAD:Pad:v1.2".to_string(),
            },
            DeviceEvent::HelloReceived {
                name: Some("Pad".to_string()),
                version: "1.2".to_string(),
            },
            DeviceEvent::Log {
                line: "[DESKTOP] Button 3 -> SINGLE".to_string(),
            },
            button("desktop", 2, "single"),
            DeviceEvent::StatusChanged {
                transport: TransportKind::Serial,
                connected: false,
                port: Some("/dev/ttyACM0".to_string()),
                message: None,
            },
        ]
    );

    // WHO + the connect-time timing push (Python writes both right after
    // connecting; TIMING is written with log=false but must still reach the pad).
    assert_eq!(
        *writes.lock().unwrap(),
        vec!["WHO".to_string(), "TIMING:25,250,450".to_string()]
    );

    // Mid-session read error backs off 1s; a failed open/rescan backs off 2s.
    let slept = durations.lock().unwrap().clone();
    assert_eq!(slept.first(), Some(&Duration::from_secs(1)));
    assert!(
        slept
            .iter()
            .all(|d| *d == Duration::from_secs(1) || *d == Duration::from_secs(2)),
        "unexpected backoff durations: {slept:?}"
    );

    shutdown_tx.send(true).expect("shutdown receiver is alive");
    while rx.recv().await.is_some() {}
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("the transport must stop after shutdown")
        .expect("the transport task must not panic");
}

/// No candidate at all: sleep 2s and rescan, never busy-spin without a sleep
/// and never emit a status event.
#[tokio::test]
async fn sleeps_two_seconds_when_no_candidate_port_is_present() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let durations = Arc::new(Mutex::new(Vec::new()));

    let transport = SerialTransport::new()
        .with_lister(Vec::new)
        .with_prober(|_port: &str| -> Result<Option<Hello>, SerialError> { unreachable!() })
        .with_opener(
            |_port: &str| -> Result<Box<dyn SerialConnection>, SerialError> { unreachable!() },
        )
        .with_sleeper(RecordingSleeper {
            durations: durations.clone(),
        });

    let handle = transport.spawn(tx, shutdown_rx);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while durations.lock().unwrap().is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the scan loop must sleep when no port is present"
        );
        tokio::task::yield_now().await;
    }

    assert!(durations
        .lock()
        .unwrap()
        .iter()
        .all(|d| *d == Duration::from_secs(2)));
    assert!(
        matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "no port means no status event"
    );

    shutdown_tx.send(true).expect("shutdown receiver is alive");
    while rx.recv().await.is_some() {}
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("the transport must stop after shutdown")
        .expect("the transport task must not panic");
}

/// A port that cannot be opened (busy / unplugged mid-scan) sleeps 2s and
/// rescans — the open-failure branch of `_run_loop`.
#[tokio::test]
async fn sleeps_two_seconds_when_opening_the_candidate_port_fails() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let durations = Arc::new(Mutex::new(Vec::new()));

    let transport = SerialTransport::new()
        .with_lister(|| vec!["/dev/ttyACM0".to_string()])
        .with_prober(|_port: &str| {
            Ok(Some(Hello {
                name: "Pad".to_string(),
                version: "1.2".to_string(),
            }))
        })
        .with_opener(|_port: &str| Err(SerialError::Busy("held by python app".to_string())))
        .with_sleeper(RecordingSleeper {
            durations: durations.clone(),
        });

    let handle = transport.spawn(tx, shutdown_rx);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while durations.lock().unwrap().is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the open-failure branch must sleep before retrying"
        );
        tokio::task::yield_now().await;
    }

    assert!(durations
        .lock()
        .unwrap()
        .iter()
        .all(|d| *d == Duration::from_secs(2)));
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));

    shutdown_tx.send(true).expect("shutdown receiver is alive");
    while rx.recv().await.is_some() {}
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("the transport must stop after shutdown")
        .expect("the transport task must not panic");
}

/// Shutdown is cooperative: a transport spawned with an already-signalled watch
/// exits without touching the (empty) port list.
#[tokio::test]
async fn stops_immediately_when_shutdown_is_already_signalled() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    shutdown_tx.send(true).expect("shutdown receiver is alive");

    let scanned = Arc::new(AtomicBool::new(false));
    let transport = SerialTransport::new()
        .with_lister({
            let scanned = scanned.clone();
            move || {
                scanned.store(true, Ordering::SeqCst);
                Vec::new()
            }
        })
        .with_prober(|_port: &str| -> Result<Option<Hello>, SerialError> { unreachable!() })
        .with_opener(
            |_port: &str| -> Result<Box<dyn SerialConnection>, SerialError> { unreachable!() },
        )
        .with_sleeper(RecordingSleeper {
            durations: Arc::new(Mutex::new(Vec::new())),
        });

    let handle = transport.spawn(tx, shutdown_rx);
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("a pre-signalled shutdown must stop the transport")
        .expect("the transport task must not panic");

    assert!(!scanned.load(Ordering::SeqCst));
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            | Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
    ));
}

/// The old M0 module path keeps working (thin re-export of `device::protocol`).
#[test]
fn m0_serial_spike_paths_still_resolve() {
    let hello = octopad_core::serial_spike::parse_hello("HELLO:OCTOPAD:Pad:v1")
        .expect("the M0 path must keep parsing");
    assert_eq!(hello.name, "Pad");
    assert!(
        octopad_core::serial_spike::candidate_ports(vec!["/dev/ttyUSB0".to_string()])
            .contains(&"/dev/ttyUSB0".to_string())
    );
}
