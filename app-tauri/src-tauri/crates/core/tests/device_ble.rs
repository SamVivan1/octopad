//! Task 6: BLE transport — stream reassembly, reconnect backoff and the
//! notify loop.
//!
//! The pure half of the layer is exercised without any adapter:
//!
//! - [`split_events`] is the exact port of the Python `_split_events` /
//!   `_LINE_PATTERN` pair the two BLE bridges use to split a notification
//!   stream that carries no line delimiter (`[DESKTOP] Button 3 -> SINGLE`,
//!   `MODE: …`, `HELLO: …`), including the "keep the partial tail" rule;
//! - [`Backoff`] mirrors Python `_RECONNECT_S = 3.0` doubling up to
//!   `_RECONNECT_MAX_S = 15.0`, reset on every successful connect;
//! - device classification (UUID first, default-name fallback — Review Focus
//!   Input 5), moved out of the M0 spike so it needs no adapter either.
//!
//! The transport loop itself is driven through an injected connector /
//! connection / sleeper, so the reconnect flow (Review Focus Input 4: a BLE
//! drop emits disconnected, backs off, reconnects and re-reads TX) is
//! exercised without hardware and without wall-clock delays.

use std::time::Duration;

use octopad_core::device::backoff::Backoff;
use octopad_core::device::protocol::{classify_device, split_events, AdvertisedDevice};
use octopad_core::proto;

// ---------------------------------------------------------------------------
// Stream reassembly (`split_events` == Python `_split_events`/`_LINE_PATTERN`)
// ---------------------------------------------------------------------------

#[test]
fn split_events_reassembles_a_hello_fragmented_across_three_notifications() {
    let mut pending = String::new();

    pending.push_str("HE");
    assert!(split_events(&mut pending).is_empty());
    assert_eq!(pending, "HE"); // partial token stays buffered

    pending.push_str("LL");
    assert!(split_events(&mut pending).is_empty());
    assert_eq!(pending, "HELL");

    pending.push_str("O:OCTOPAD:My Pad:v2.6");
    assert_eq!(
        split_events(&mut pending),
        vec!["HELLO:OCTOPAD:My Pad:v2.6"]
    );
    assert!(pending.is_empty());
}

#[test]
fn split_events_keeps_a_partial_event_line_buffered() {
    let mut pending = String::from("[DESKTOP] Button 3 -> SIN");
    assert!(split_events(&mut pending).is_empty());
    assert_eq!(pending, "[DESKTOP] Button 3 -> SIN");

    pending.push_str("GLE");
    assert_eq!(
        split_events(&mut pending),
        vec!["[DESKTOP] Button 3 -> SINGLE"]
    );
    assert!(pending.is_empty());
}

#[test]
fn split_events_extracts_events_mode_and_hello_lines_in_stream_order() {
    let mut pending = String::from(
        "[DESKTOP] Button 3 -> SINGLE\nMODE: HOME ASSISTANT\nHELLO:OCTOPAD:Pad:v1.2\n",
    );
    assert_eq!(
        split_events(&mut pending),
        vec![
            "[DESKTOP] Button 3 -> SINGLE",
            "MODE: HOME ASSISTANT",
            "HELLO:OCTOPAD:Pad:v1.2",
        ]
    );
    // Python keeps `text[last_end:]` — the final newline stays buffered.
    assert_eq!(pending, "\n");
}

#[test]
fn split_events_keeps_trailing_noise_until_the_next_line_or_marker() {
    // `[^\n\[]*` is greedy but stops at the next `[`, so the noise after an
    // event travels with it while the next marker starts a new match.
    let mut pending = String::from("[DESKTOP] Button 3 -> SINGLE then[HA] Button 1 -> HOLD");
    assert_eq!(
        split_events(&mut pending),
        vec!["[DESKTOP] Button 3 -> SINGLE then", "[HA] Button 1 -> HOLD"]
    );
    assert!(pending.is_empty());
}

#[test]
fn split_events_retains_text_after_a_mode_line_like_python() {
    // The MODE alternative carries no trailing group: Python appends only
    // `match.group(0)` and keeps the rest buffered.
    let mut pending = String::from("MODE: DESKTOP extra");
    assert_eq!(split_events(&mut pending), vec!["MODE: DESKTOP"]);
    assert_eq!(pending, " extra");
}

#[test]
fn split_events_matches_case_insensitively_and_preserves_the_original_text() {
    let mut pending = String::from("hello:octopad:Pad:v1");
    assert_eq!(split_events(&mut pending), vec!["hello:octopad:Pad:v1"]);
    assert!(pending.is_empty());

    // `MODE:\s*` tolerates any (including no) whitespace.
    let mut pending = String::from("mode:  home assistant");
    assert_eq!(split_events(&mut pending), vec!["mode:  home assistant"]);
    assert!(pending.is_empty());
}

#[test]
fn split_events_retains_a_buffer_without_a_complete_marker() {
    for text in ["HEL", "MODE: HO", "[DESKTOP] Button 3 -> SIN", "HELLO"] {
        let mut pending = String::from(text);
        assert!(
            split_events(&mut pending).is_empty(),
            "{text:?} must stay buffered until it is complete"
        );
        assert_eq!(pending, text, "{text:?} must not be dropped or altered");
    }
}

#[test]
fn split_events_matches_the_python_regex_on_awkward_inputs() {
    // Every expectation below was produced by running the Python
    // `_LINE_PATTERN`/`_split_events` pair on the same input.
    let cases: &[(&str, &[&str], &str)] = &[
        // `\s*` accepts no whitespace at all.
        ("MODE:desktop", &["MODE:desktop"], ""),
        ("MODE:  HOME ASSISTANT", &["MODE:  HOME ASSISTANT"], ""),
        // `MODE:\s*(HOME ASSISTANT|DESKTOP)` — the bare abbreviation is noise.
        ("MODE: HOME", &[], "MODE: HOME"),
        ("  MODE: HA  ", &[], "  MODE: HA  "),
        ("MODE: HOME ASSISTAN", &[], "MODE: HOME ASSISTAN"),
        // A match may start anywhere (Python `finditer`, not `match`).
        (
            "x[DESKTOP] Button 1 -> SINGLE",
            &["[DESKTOP] Button 1 -> SINGLE"],
            "",
        ),
        // `\s+` is required on both sides of `->`.
        (
            "[DESKTOP]button 1->single",
            &[],
            "[DESKTOP]button 1->single",
        ),
        (
            "[DESKTOP] Button 1->single",
            &[],
            "[DESKTOP] Button 1->single",
        ),
        (
            "[DESKTOP] Button 1   ->   DOUBLE",
            &["[DESKTOP] Button 1   ->   DOUBLE"],
            "",
        ),
        // `HELLO:` alone matches; `[^\n\[]*` stops at a newline or `[`.
        ("HELLO:", &["HELLO:"], ""),
        ("HELLO:OCTOPAD:A[BB", &["HELLO:OCTOPAD:A"], "[BB"),
        (
            "HELLO:OCTOPAD:Pad:v1.2\nMODE: HA extra",
            &["HELLO:OCTOPAD:Pad:v1.2"],
            "\nMODE: HA extra",
        ),
        // A later alternative wins where the earlier one cannot match.
        ("junk HELLO:a MODE: HA", &["HELLO:a MODE: HA"], ""),
        ("\n\nHELLO:x", &["HELLO:x"], ""),
        // Case-insensitivity is ASCII-only (Python `re.IGNORECASE` here).
        ("H\u{c9}LLO:x", &[], "H\u{c9}LLO:x"),
        (
            "HELLO:\u{e9}\u{e9}\u{e9}[",
            &["HELLO:\u{e9}\u{e9}\u{e9}"],
            "[",
        ),
        // Trailing noise is kept, but the line stops at the next `[`/newline.
        (
            "[DESKTOP] Button 1 -> SINGLE\n\n[HA] Button 0 -> HOLD",
            &["[DESKTOP] Button 1 -> SINGLE", "[HA] Button 0 -> HOLD"],
            "",
        ),
        (
            "HELLO:a\nb[HA] Button 1 -> HOLD",
            &["HELLO:a", "[HA] Button 1 -> HOLD"],
            "",
        ),
    ];

    for (input, events, remainder) in cases {
        let mut buffer = String::from(*input);
        assert_eq!(
            split_events(&mut buffer),
            events.to_vec(),
            "events for {input:?}"
        );
        assert_eq!(buffer, *remainder, "remainder for {input:?}");
    }
}

// ---------------------------------------------------------------------------
// Reconnect backoff (Python `_RECONNECT_S`/`_RECONNECT_MAX_S`)
// ---------------------------------------------------------------------------

#[test]
fn backoff_doubles_to_the_cap_and_resets_to_three_seconds() {
    let mut backoff = Backoff::ble();
    assert_eq!(backoff.current(), Duration::from_secs(3));

    assert_eq!(backoff.next_delay(), Duration::from_secs(3));
    assert_eq!(backoff.next_delay(), Duration::from_secs(6));
    assert_eq!(backoff.next_delay(), Duration::from_secs(12));
    assert_eq!(backoff.next_delay(), Duration::from_secs(15));
    assert_eq!(backoff.next_delay(), Duration::from_secs(15));

    // A successful connect resets the ladder (Python sets `_reconnect_delay`).
    backoff.reset();
    assert_eq!(backoff.next_delay(), Duration::from_secs(3));
}

#[test]
fn backoff_uses_the_configured_bounds() {
    let mut backoff = Backoff::new(Duration::from_secs(1), Duration::from_secs(5));
    assert_eq!(backoff.next_delay(), Duration::from_secs(1));
    assert_eq!(backoff.next_delay(), Duration::from_secs(2));
    assert_eq!(backoff.next_delay(), Duration::from_secs(4));
    assert_eq!(backoff.next_delay(), Duration::from_secs(5));
    assert_eq!(backoff.next_delay(), Duration::from_secs(5));

    assert_eq!(Backoff::default(), Backoff::ble());
}

// ---------------------------------------------------------------------------
// Device classification (UUID first, default-name fallback)
// ---------------------------------------------------------------------------

fn advertised(name: Option<&str>, uuids: &[&str]) -> AdvertisedDevice {
    AdvertisedDevice {
        name: name.map(str::to_string),
        address: "aa:bb:cc:dd:ee:ff".to_string(),
        service_uuids: uuids.iter().map(|uuid| uuid.to_string()).collect(),
    }
}

#[test]
fn classification_accepts_a_renamed_pad_by_service_uuid() {
    let discovered = classify_device(&advertised(Some("Desk Pad"), &[proto::SERVICE_UUID]))
        .expect("the advertised service UUID identifies the pad whatever its name");
    assert!(discovered.matched_by_uuid);
    assert_eq!(discovered.name, "Desk Pad");
    assert_eq!(discovered.address, "aa:bb:cc:dd:ee:ff");

    // Legacy Nordic-UART firmware is accepted too.
    let legacy = classify_device(&advertised(None, &[proto::LEGACY_SERVICE_UUID]))
        .expect("legacy service UUID must match");
    assert!(legacy.matched_by_uuid);
}

#[test]
fn classification_falls_back_to_the_default_name_without_a_matching_uuid() {
    let discovered = classify_device(&advertised(Some("  OctoPad  "), &[]))
        .expect("the default name is the fallback when no UUID is advertised");
    assert!(!discovered.matched_by_uuid);
    assert_eq!(discovered.name, "OctoPad");
}

#[test]
fn classification_rejects_unrelated_devices() {
    assert!(classify_device(&advertised(
        Some("Keyboard"),
        &["00001234-0000-1000-8000-00805f9b34fb"]
    ))
    .is_none());
    // Characteristic UUIDs never identify a pad.
    assert!(classify_device(&advertised(Some("Keyboard"), &[proto::TX_UUID])).is_none());
    assert!(classify_device(&advertised(Some("Keyboard"), &[])).is_none());
}

// ---------------------------------------------------------------------------
// Transport loop with injected fakes
// ---------------------------------------------------------------------------

#[cfg(feature = "ble")]
mod transport {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use octopad_core::device::ble_transport::{
        BleConnection, BleConnector, BleTransport, BoxFuture, Sleeper, STATUS_CONNECTED,
        STATUS_DISCONNECTED, STATUS_SEARCHING,
    };
    use octopad_core::device::protocol::BleError;
    use octopad_core::device::{DeviceEvent, TransportKind};
    use tokio::sync::mpsc::error::TryRecvError;

    const ADDRESS: &str = "aa:bb:cc:dd:ee:ff";

    fn searching() -> DeviceEvent {
        DeviceEvent::StatusChanged {
            transport: TransportKind::Ble,
            connected: false,
            port: None,
            message: Some(STATUS_SEARCHING.to_string()),
        }
    }

    fn connected(address: &str) -> DeviceEvent {
        DeviceEvent::StatusChanged {
            transport: TransportKind::Ble,
            connected: true,
            port: Some(address.to_string()),
            message: Some(STATUS_CONNECTED.to_string()),
        }
    }

    fn disconnected(address: &str) -> DeviceEvent {
        DeviceEvent::StatusChanged {
            transport: TransportKind::Ble,
            connected: false,
            port: Some(address.to_string()),
            message: Some(STATUS_DISCONNECTED.to_string()),
        }
    }

    /// Zero-delay async sleeper: records the requested durations and yields, so
    /// the transport task stays schedulable on the current-thread test runtime
    /// (no wall-clock waits, no tight loop).
    struct RecordingSleeper {
        durations: Arc<Mutex<Vec<Duration>>>,
    }

    impl Sleeper for RecordingSleeper {
        fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()> {
            self.durations.lock().unwrap().push(duration);
            Box::pin(tokio::task::yield_now())
        }
    }

    /// Scripted stand-in for a connected pad: replays the one-shot TX read and
    /// the notification chunks, records writes, and reports "closed".
    struct FakeConnection {
        tx_read: Result<Vec<u8>, BleError>,
        notifications: VecDeque<Vec<u8>>,
        notify_error: Option<BleError>,
        can_write: bool,
        writes: Arc<Mutex<Vec<String>>>,
        closed: Arc<AtomicBool>,
    }

    impl FakeConnection {
        fn new(writes: Arc<Mutex<Vec<String>>>, closed: Arc<AtomicBool>) -> Self {
            Self {
                tx_read: Ok(Vec::new()),
                notifications: VecDeque::new(),
                notify_error: None,
                can_write: true,
                writes,
                closed,
            }
        }

        fn with_tx_read(mut self, bytes: &[u8]) -> Self {
            self.tx_read = Ok(bytes.to_vec());
            self
        }

        fn without_tx_read(mut self) -> Self {
            self.tx_read = Err(BleError::Other("TX read rejected".to_string()));
            self
        }

        fn with_notifications(mut self, chunks: &[&[u8]]) -> Self {
            self.notifications = chunks.iter().map(|chunk| chunk.to_vec()).collect();
            self
        }

        fn with_notify_error(mut self, err: BleError) -> Self {
            self.notify_error = Some(err);
            self
        }

        fn without_rx(mut self) -> Self {
            self.can_write = false;
            self
        }

        fn into_connection(self) -> Box<dyn BleConnection> {
            Box::new(self)
        }
    }

    impl BleConnection for FakeConnection {
        fn address(&self) -> String {
            ADDRESS.to_string()
        }

        fn can_write(&self) -> bool {
            self.can_write
        }

        fn read_tx(&mut self) -> BoxFuture<'_, Result<Vec<u8>, BleError>> {
            let result = std::mem::replace(&mut self.tx_read, Ok(Vec::new()));
            Box::pin(async move { result })
        }

        fn start_notify(&mut self) -> BoxFuture<'_, Result<(), BleError>> {
            Box::pin(async { Ok(()) })
        }

        fn read_notify(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, BleError>> {
            Box::pin(async move {
                if let Some(chunk) = self.notifications.pop_front() {
                    return Ok(Some(chunk));
                }
                if let Some(err) = self.notify_error.take() {
                    return Err(err);
                }
                // Idle: yield to the runtime instead of spinning on a
                // ready-forever future (a real notify stream blocks between
                // events; the production read has a read-slice timeout).
                tokio::time::sleep(Duration::from_millis(1)).await;
                Ok(None)
            })
        }

        fn write_line(&mut self, line: &str) -> BoxFuture<'_, Result<(), BleError>> {
            self.writes.lock().unwrap().push(line.to_string());
            let accepted = self.can_write;
            Box::pin(async move {
                if accepted {
                    Ok(())
                } else {
                    Err(BleError::Other("RX characteristic not found".to_string()))
                }
            })
        }

        fn close(&mut self) -> BoxFuture<'_, ()> {
            self.closed.store(true, Ordering::SeqCst);
            Box::pin(async {})
        }
    }

    /// Connector that hands out scripted sessions in order and reports "no pad
    /// found" once the script is exhausted.
    fn scripted_connector(script: Vec<Box<dyn BleConnection>>) -> BleConnector {
        let script = Arc::new(Mutex::new(VecDeque::from(script)));
        Box::new(move || {
            let next = script.lock().unwrap().pop_front();
            Box::pin(async move { Ok(next) })
        })
    }

    async fn next_event(rx: &mut tokio::sync::mpsc::Receiver<DeviceEvent>) -> DeviceEvent {
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("the transport must not stall on wall-clock waits")
            .expect("the channel stays open while the transport runs")
    }

    async fn collect_until(
        rx: &mut tokio::sync::mpsc::Receiver<DeviceEvent>,
        mut done: impl FnMut(&DeviceEvent) -> bool,
    ) -> Vec<DeviceEvent> {
        let mut events = Vec::new();
        loop {
            let event = next_event(rx).await;
            let finished = done(&event);
            events.push(event);
            if finished {
                return events;
            }
        }
    }

    /// Review Focus Input 4: connect → HELLO (TX read once, fed through the
    /// reassembler) → fragmented button event → MODE line → link drop; then a
    /// reconnect that re-reads TX and re-emits the connected status after a 3s
    /// backoff (the ladder resets on every successful connect).
    #[tokio::test]
    async fn emits_connect_hello_button_and_disconnect_sequence_then_reconnects() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let first_writes = Arc::new(Mutex::new(Vec::new()));
        let second_writes = Arc::new(Mutex::new(Vec::new()));
        let first_closed = Arc::new(AtomicBool::new(false));
        let second_closed = Arc::new(AtomicBool::new(false));
        let durations = Arc::new(Mutex::new(Vec::new()));

        let first = FakeConnection::new(first_writes.clone(), first_closed.clone())
            .with_tx_read(b"HELLO:OCTOPAD:My Pad:v2.6\n")
            .with_notifications(&[
                b"[DESKTOP] Button 3 -> SIN", // fragment: not a complete line yet
                b"GLE",
                b"\nMODE: HOME ASSISTANT",
            ])
            .with_notify_error(BleError::Other("link dropped".to_string()));
        let second = FakeConnection::new(second_writes.clone(), second_closed.clone());

        let transport = BleTransport::new()
            .with_connector(scripted_connector(vec![
                first.into_connection(),
                second.into_connection(),
            ]))
            .with_sleeper(RecordingSleeper {
                durations: durations.clone(),
            })
            .with_timing_command(Some("TIMING:25,250,450".to_string()));
        let commands = transport.command_handle();
        let task = transport.spawn(tx, shutdown_rx);

        let mut connects = 0;
        let events = collect_until(&mut rx, |event| {
            if matches!(
                event,
                DeviceEvent::StatusChanged {
                    connected: true,
                    ..
                }
            ) {
                connects += 1;
            }
            connects == 2
        })
        .await;

        assert_eq!(
            events,
            vec![
                searching(),
                connected(ADDRESS),
                // TX read → reassembled HELLO → log + identity.
                DeviceEvent::Log {
                    line: "HELLO:OCTOPAD:My Pad:v2.6".to_string(),
                },
                DeviceEvent::HelloReceived {
                    name: Some("My Pad".to_string()),
                    version: "2.6".to_string(),
                },
                // The three notification fragments reassemble into one event.
                DeviceEvent::Log {
                    line: "[DESKTOP] Button 3 -> SINGLE".to_string(),
                },
                DeviceEvent::ButtonEvent {
                    mode: "desktop".to_string(),
                    index: 2,
                    trigger: "single".to_string(),
                },
                DeviceEvent::Log {
                    line: "MODE: HOME ASSISTANT".to_string(),
                },
                disconnected(ADDRESS),
                // Each retry re-announces the scan (Python `_main`).
                searching(),
                connected(ADDRESS),
            ]
        );

        // Python `sync_timings` pushes TIMING with `log=False`: both connects
        // write it and no `[TX-BLE]` line appears.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while first_writes.lock().unwrap().is_empty() || second_writes.lock().unwrap().is_empty() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "TIMING must be pushed right after every connect"
            );
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(
            *first_writes.lock().unwrap(),
            vec!["TIMING:25,250,450".to_string()]
        );
        assert_eq!(
            *second_writes.lock().unwrap(),
            vec!["TIMING:25,250,450".to_string()]
        );
        assert!(
            !events.iter().any(|event| matches!(
                event,
                DeviceEvent::Log { line } if line.starts_with("[TX-BLE]")
            )),
            "the timing push must not log a TX line"
        );

        // The drop closed the first link, reported it, and the ladder restarted
        // at 3s because the reconnect reset it.
        assert!(first_closed.load(Ordering::SeqCst));
        assert!(!second_closed.load(Ordering::SeqCst));
        assert_eq!(*durations.lock().unwrap(), vec![Duration::from_secs(3)]);

        assert!(commands.is_connected());
        assert!(commands.send_command("WHO", true).await);

        // Stop the event consumer first so a stalled receiver can never block
        // the transport's next send (the channel is bounded).
        drop(rx);
        let _ = shutdown_tx.send(true);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the transport must stop after shutdown")
            .expect("the transport task must not panic");
        assert!(!commands.is_connected());
    }

    /// A rejected TX read is not fatal (notify-only pads): the pad still
    /// connects, no error is surfaced, and commands keep flowing.
    #[tokio::test]
    async fn a_failed_tx_read_does_not_abort_the_session() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let writes = Arc::new(Mutex::new(Vec::new()));

        let connection =
            FakeConnection::new(writes.clone(), Arc::new(AtomicBool::new(false))).without_tx_read();
        let transport = BleTransport::new()
            .with_connector(scripted_connector(vec![connection.into_connection()]))
            .with_sleeper(RecordingSleeper {
                durations: Arc::new(Mutex::new(Vec::new())),
            })
            .with_timing_command(None);
        let commands = transport.command_handle();
        let task = transport.spawn(tx, shutdown_rx);

        let events = collect_until(&mut rx, |event| {
            matches!(
                event,
                DeviceEvent::StatusChanged {
                    connected: true,
                    ..
                }
            )
        })
        .await;
        assert_eq!(events, vec![searching(), connected(ADDRESS)]);
        assert!(commands.is_connected());
        assert!(commands.send_command("WHO", true).await);

        drop(rx);
        let _ = shutdown_tx.send(true);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the transport must stop after shutdown")
            .expect("the transport task must not panic");
    }

    /// `send_command` queues to the RX characteristic and logs `[TX-BLE] …`
    /// only when `log=true` (Python `_write`).
    #[tokio::test]
    async fn send_command_writes_the_rx_characteristic_and_logs_the_tx_line() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let writes = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let durations = Arc::new(Mutex::new(Vec::new()));

        let connection = FakeConnection::new(writes.clone(), closed.clone());
        let transport = BleTransport::new()
            .with_connector(scripted_connector(vec![connection.into_connection()]))
            .with_sleeper(RecordingSleeper {
                durations: durations.clone(),
            })
            .with_timing_command(None);
        let commands = transport.command_handle();
        let task = transport.spawn(tx, shutdown_rx);

        let events = collect_until(&mut rx, |event| {
            matches!(
                event,
                DeviceEvent::StatusChanged {
                    connected: true,
                    ..
                }
            )
        })
        .await;
        assert_eq!(events, vec![searching(), connected(ADDRESS)]);
        assert!(commands.is_connected());

        // log=true: the write reaches RX and a `[TX-BLE]` line is emitted.
        assert!(commands.send_command("NAME:My Pad", true).await);
        let logged = collect_until(
            &mut rx,
            |event| matches!(event, DeviceEvent::Log { line } if line.starts_with("[TX-BLE]")),
        )
        .await;
        assert_eq!(
            logged,
            vec![DeviceEvent::Log {
                line: "[TX-BLE] NAME:My Pad".to_string(),
            }]
        );

        // log=false (Python `sync_timings`): the write happens without a log.
        assert!(commands.send_command("TIMING:25,250,450", false).await);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while writes.lock().unwrap().len() < 2 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the queued command must reach the RX characteristic"
            );
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(
            *writes.lock().unwrap(),
            vec!["NAME:My Pad".to_string(), "TIMING:25,250,450".to_string()]
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "a log=false write must not emit a log line"
        );

        // Stop the event consumer first so a stalled receiver can never block
        // the transport's next send (the channel is bounded).
        drop(rx);
        let _ = shutdown_tx.send(true);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the transport must stop after shutdown")
            .expect("the transport task must not panic");
        assert!(!commands.is_connected());
        assert!(!commands.send_command("WHO", true).await);
    }

    /// A pad without an RX characteristic still connects, but commands are
    /// refused (Python `_rx_char is None` → `send_command` returns False).
    #[tokio::test]
    async fn send_command_is_refused_without_an_rx_characteristic() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let writes = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(AtomicBool::new(false));

        let connection = FakeConnection::new(writes.clone(), closed).without_rx();
        let transport = BleTransport::new()
            .with_connector(scripted_connector(vec![connection.into_connection()]))
            .with_sleeper(RecordingSleeper {
                durations: Arc::new(Mutex::new(Vec::new())),
            })
            .with_timing_command(None);
        let commands = transport.command_handle();
        let task = transport.spawn(tx, shutdown_rx);

        collect_until(&mut rx, |event| {
            matches!(
                event,
                DeviceEvent::StatusChanged {
                    connected: true,
                    ..
                }
            )
        })
        .await;

        assert!(commands.is_connected());
        assert!(!commands.send_command("WHO", true).await);
        assert!(writes.lock().unwrap().is_empty());

        // Stop the event consumer first so a stalled receiver can never block
        // the transport's next send (the channel is bounded).
        drop(rx);
        let _ = shutdown_tx.send(true);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the transport must stop after shutdown")
            .expect("the transport task must not panic");
    }

    /// Review Focus Input 1 analog: another client owns the pad. Every attempt
    /// surfaces a structured `Error` and is followed by a recorded backoff
    /// (3s → 6s → 12s → 15s → 15s), never a tight retry loop.
    #[tokio::test]
    async fn busy_device_emits_a_structured_error_and_backs_off_without_a_tight_loop() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let durations = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));

        let connector: BleConnector = Box::new({
            let attempts = attempts.clone();
            move || {
                attempts.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Err(BleError::Busy("GATT server is busy".to_string())) })
            }
        });

        let transport = BleTransport::new()
            .with_connector(connector)
            .with_sleeper(RecordingSleeper {
                durations: durations.clone(),
            })
            .with_timing_command(None);
        let task = transport.spawn(tx, shutdown_rx);

        let mut errors = 0;
        let events = collect_until(&mut rx, |event| {
            if matches!(event, DeviceEvent::Error { .. }) {
                errors += 1;
            }
            errors == 5
        })
        .await;

        for event in &events {
            if let DeviceEvent::Error { message } = event {
                assert!(
                    message.contains("busy"),
                    "the error must stay structured and name the cause: {message:?}"
                );
            }
        }
        assert!(
            !events.iter().any(|event| matches!(
                event,
                DeviceEvent::StatusChanged {
                    connected: true,
                    ..
                }
            )),
            "a busy device is never reported as connected"
        );

        // The error is emitted just before the backoff, so wait for it to land.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while durations.lock().unwrap().len() < 5 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "every failed attempt must be followed by a backoff (no tight loop)"
            );
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        let slept = durations.lock().unwrap().clone();
        assert_eq!(
            &slept[..5],
            &[
                Duration::from_secs(3),
                Duration::from_secs(6),
                Duration::from_secs(12),
                Duration::from_secs(15),
                Duration::from_secs(15),
            ]
        );
        assert!(
            attempts.load(Ordering::SeqCst) <= slept.len() + 1,
            "every failed attempt must be followed by a backoff (no tight loop)"
        );

        // Stop the event consumer first so a stalled receiver can never block
        // the transport's next send (the channel is bounded).
        drop(rx);
        let _ = shutdown_tx.send(true);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the transport must stop after shutdown")
            .expect("the transport task must not panic");
    }

    /// No pad in range: status messages only (no `Error`), 3s → 6s → 12s
    /// backoff (Python `_find_device` returning None).
    #[tokio::test]
    async fn no_pad_in_range_reports_searching_and_backs_off_without_errors() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let durations = Arc::new(Mutex::new(Vec::new()));

        let connector: BleConnector = Box::new(|| Box::pin(async { Ok(None) }));
        let transport = BleTransport::new()
            .with_connector(connector)
            .with_sleeper(RecordingSleeper {
                durations: durations.clone(),
            })
            .with_timing_command(None);
        let task = transport.spawn(tx, shutdown_rx);

        let mut searches = 0;
        let events = collect_until(&mut rx, |event| {
            if matches!(
                event,
                DeviceEvent::StatusChanged {
                    connected: false,
                    message: Some(message),
                    ..
                } if message == STATUS_SEARCHING
            ) {
                searches += 1;
            }
            searches == 3
        })
        .await;

        assert_eq!(events, vec![searching(), searching(), searching()]);
        let slept = durations.lock().unwrap().clone();
        assert_eq!(
            &slept[..2],
            &[Duration::from_secs(3), Duration::from_secs(6)]
        );

        // Stop the event consumer first so a stalled receiver can never block
        // the transport's next send (the channel is bounded).
        drop(rx);
        let _ = shutdown_tx.send(true);
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the transport must stop after shutdown")
            .expect("the transport task must not panic");
    }

    /// Cooperative shutdown: a transport spawned with an already-signalled
    /// watch exits without scanning and without emitting anything.
    #[tokio::test]
    async fn stops_immediately_when_shutdown_is_already_signalled() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        shutdown_tx.send(true).expect("shutdown receiver is alive");

        let attempted = Arc::new(AtomicBool::new(false));
        let connector: BleConnector = Box::new({
            let attempted = attempted.clone();
            move || {
                attempted.store(true, Ordering::SeqCst);
                Box::pin(async { Ok(None) })
            }
        });

        let transport = BleTransport::new()
            .with_connector(connector)
            .with_sleeper(RecordingSleeper {
                durations: Arc::new(Mutex::new(Vec::new())),
            })
            .with_timing_command(None);
        let task = transport.spawn(tx, shutdown_rx);

        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("a pre-signalled shutdown must stop the transport")
            .expect("the transport task must not panic");

        assert!(!attempted.load(Ordering::SeqCst));
        assert!(matches!(
            rx.try_recv(),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected)
        ));
    }

    /// Hardware smoke test: the real btleplug path must never panic, only
    /// report structured events. Ignored by default — no adapter in CI and no
    /// pad guaranteed.
    #[tokio::test]
    #[ignore = "requires a BLE adapter (and a powered OctoPad for a connection)"]
    async fn ble_transport_with_hardware_does_not_panic() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let task = BleTransport::new().spawn(tx, shutdown_rx);

        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
                Ok(Some(event)) => eprintln!("ble event: {event:?}"),
                Ok(None) => break,
                Err(_) => {}
            }
        }

        // Stop the event consumer first so a stalled receiver can never block
        // the transport's next send (the channel is bounded).
        drop(rx);
        let _ = shutdown_tx.send(true);
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("shutdown must stop the transport")
            .expect("the transport task must not panic");
    }
}
