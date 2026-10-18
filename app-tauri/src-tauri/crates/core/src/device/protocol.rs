//! Device protocol helpers shared by every transport.
//!
//! This module is the single home of the wire-level helpers the M0 serial
//! spike introduced (`linux-app/serial_listener.py` is the source of truth) and
//! adds the structured event line parser the transports feed to the UI:
//!
//! - [`list_ports`] / [`probe_hello`] — USB-serial enumeration and the minimal
//!   `WHO -> HELLO:OCTOPAD:<name>:v<version>` handshake (moved from the M0
//!   `serial_spike` module, which now re-exports these paths);
//! - [`parse_hello`] — identity-line parsing, mirroring
//!   `parse_hello` in `tests/test_parsers.py` (brand gate, default name,
//!   optional version, case-insensitive);
//! - [`parse_event_line`] — the `EVENT_PATTERN` semantics of
//!   `linux-app/serial_listener.py::parse_event_line`: mode `desktop|ha`,
//!   **0-based** button index, trigger `single|double|hold`, case-insensitive;
//! - [`candidate_ports`] — the port shapes the apps scan
//!   (`/dev/ttyUSB*`, `/dev/ttyACM*` on Linux, `COM*` on Windows).
//!
//! The serial-specific I/O helpers are compiled only with the `serial`
//! feature; everything else (HELLO/event parsing, candidate filtering) is pure
//! and always available so the BLE transport can reuse it.

use std::fmt;

use super::DeviceEvent;

/// Brand tag inside the identity line, mirroring `BRAND_TAG` in the Python
/// listeners. Only lines carrying this tag are genuine OctoPad HELLOs.
pub const BRAND_TAG: &str = "OCTOPAD";

/// Default name when the pad reports an empty one (Python:
/// `name.strip() or "OctoPad"`).
pub const DEFAULT_DEVICE_NAME: &str = "OctoPad";

/// Identity request written to the pad (firmware: `WHO -> HELLO:...`), without
/// the line terminator the transports add.
pub const WHO_COMMAND: &str = "WHO";

/// Parsed firmware identity line: `HELLO:OCTOPAD:<name>:v<version>`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Hello {
    /// User-configured device name (defaults to [`DEFAULT_DEVICE_NAME`]).
    pub name: String,
    /// Firmware version, empty when the line carries none.
    pub version: String,
}

/// Structured serial errors — the transports never panic on hardware failures
/// and callers can distinguish "busy" (port owned by another client, e.g. the
/// running Python app — Review Focus Input 1) and permission problems from
/// other failures without string matching.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum SerialError {
    /// The port reports busy/locked — most likely owned by another process.
    Busy(String),
    /// The current user is not allowed to open the port.
    PermissionDenied(String),
    /// The port does not exist (read phase; open-phase misses are skips).
    NotFound(String),
    /// Any other open/read failure, with the underlying reason.
    Other(String),
}

impl fmt::Display for SerialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SerialError::Busy(msg) => write!(f, "serial port busy: {msg}"),
            SerialError::PermissionDenied(msg) => write!(f, "serial permission denied: {msg}"),
            SerialError::NotFound(msg) => write!(f, "serial port not found: {msg}"),
            SerialError::Other(msg) => write!(f, "serial error: {msg}"),
        }
    }
}

impl std::error::Error for SerialError {}

/// Pop one complete line (`\n`-terminated, optional trailing `\r`) from
/// `pending`, mirroring the Python `readline()` loop. An incomplete tail stays
/// buffered.
pub fn take_line(pending: &mut Vec<u8>) -> Option<String> {
    let idx = pending.iter().position(|&b| b == b'\n')?;
    let line = pending.drain(..=idx).collect::<Vec<u8>>();
    let mut text = String::from_utf8_lossy(&line[..idx]).into_owned(); // drop the '\n'
    if text.ends_with('\r') {
        text.pop();
    }
    Some(text)
}

// ---------------------------------------------------------------------------
// HELLO parsing
// ---------------------------------------------------------------------------

/// Parse an identity line, mirroring Python:
///
/// ```python
/// HELLO_PATTERN = re.compile(
///     r'HELLO:([A-Za-z0-9_-]+):([^:\n]*)(?::v?([0-9][0-9A-Za-z._-]*))?',
///     re.IGNORECASE)
/// # then: match.group(1).upper() == BRAND_TAG, name.strip() or "OctoPad"
/// ```
///
/// Returns `None` for anything that is not a genuine OctoPad identity line.
pub fn parse_hello(line: &str) -> Option<Hello> {
    // ASCII-only lowercasing keeps byte offsets identical between `lower`
    // (the search alphabet) and `line` (the source of every slice).
    let lower = line.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find("hello:") {
        let rest_at = from + rel + "hello:".len();
        from = rest_at; // `.search` semantics: on a pattern miss, keep looking
        match brand_and_body(line, rest_at) {
            None => continue,
            Some((brand, body)) => {
                if !brand.eq_ignore_ascii_case(BRAND_TAG) {
                    // Python takes the *first* HELLO match and rejects foreign
                    // brands outright — no fallback to a later line.
                    return None;
                }
                return Some(parse_identity(body));
            }
        }
    }
    None
}

/// The pattern fragment `([A-Za-z0-9_-]+):` right after `HELLO:` — returns the
/// brand and everything after its colon, or `None` when the pattern does not
/// fit at `start`.
fn brand_and_body(line: &str, start: usize) -> Option<(&str, &str)> {
    let rest = line.get(start..)?;
    let brand_len = rest
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        .count();
    if brand_len == 0 {
        return None; // group(1) needs at least one character
    }
    // The run is pure ASCII, so slicing at `brand_len` is a char boundary.
    let brand = &rest[..brand_len];
    let body = rest.get(brand_len..)?.strip_prefix(':')?;
    Some((brand, body))
}

/// Parse `([^:\n]*)(?::v?([0-9][0-9A-Za-z._-]*))?` after the brand colon:
/// the device name and the optional version.
fn parse_identity(body: &str) -> Hello {
    let name_end = body.find(|c| c == ':' || c == '\n').unwrap_or(body.len());
    let name = body[..name_end].trim();
    let name = if name.is_empty() {
        DEFAULT_DEVICE_NAME
    } else {
        name
    };

    // Optional `:v?<version>`; anything else after the name is ignored
    // (Python: the optional group fails to match and stays empty).
    let mut version = "";
    if let Some(after_colon) = body[name_end..].strip_prefix(':') {
        let after_v = after_colon
            .strip_prefix('v')
            .or_else(|| after_colon.strip_prefix('V'))
            .unwrap_or(after_colon);
        version = parse_version(after_v).unwrap_or("");
    }

    Hello {
        name: name.to_string(),
        version: version.to_string(),
    }
}

/// `[0-9][0-9A-Za-z._-]*` — a version token must start with a digit.
fn parse_version(after_v: &str) -> Option<&str> {
    let end = after_v
        .bytes()
        .position(|b| !(b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-'))
        .unwrap_or(after_v.len());
    let candidate = &after_v[..end];
    candidate
        .starts_with(|c: char| c.is_ascii_digit())
        .then_some(candidate)
}

// ---------------------------------------------------------------------------
// Button-event parsing
// ---------------------------------------------------------------------------

/// Parse a single event line, the exact port of Python:
///
/// ```python
/// EVENT_PATTERN = re.compile(
///     r'\[(DESKTOP|HA)\]\s+Button\s+(\d+)\s+->\s+(SINGLE|DOUBLE|HOLD)',
///     re.IGNORECASE)
/// # match = EVENT_PATTERN.search(line)
/// # {"mode": group(1).lower(), "index": int(group(2)) - 1,
/// #  "trigger": group(3).lower()}
/// ```
///
/// Returns `DeviceEvent::ButtonEvent { mode, index, trigger }` or `None` for
/// noise lines. `index` is 0-based (Python subtracts one) and the keyword is
/// matched case-insensitively anywhere in the line (`re.search` semantics).
pub fn parse_event_line(line: &str) -> Option<DeviceEvent> {
    // ASCII-only lowercasing keeps byte offsets aligned with `line`, so all
    // matching happens on `lower` while offsets stay valid for `line` too.
    let lower = line.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find('[') {
        let start = from + rel;
        from = start + 1;
        if let Some(event) = parse_event_at(&lower, start) {
            return Some(event);
        }
    }
    None
}

/// Try to match `[(DESKTOP|HA)]\s+Button\s+(\d+)\s+->\s+(SINGLE|DOUBLE|HOLD)`
/// at `start` (the byte offset of the `[` in the lowercased line).
fn parse_event_at(lower: &str, start: usize) -> Option<DeviceEvent> {
    let rest = lower.get(start..)?.strip_prefix('[')?;
    let (mode, rest) = if let Some(rest) = rest.strip_prefix("desktop]") {
        ("desktop", rest)
    } else if let Some(rest) = rest.strip_prefix("ha]") {
        ("ha", rest)
    } else {
        return None;
    };

    let rest = skip_whitespace(rest)?; // `\s+` after `]`
    let rest = rest.strip_prefix("button")?;
    let rest = skip_whitespace(rest)?; // `\s+` after `Button`
    let digits = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let index: i64 = rest[..digits].parse().ok()?;
    let rest = skip_whitespace(&rest[digits..])?; // `\s+` before `->`
    let rest = rest.strip_prefix("->")?;
    let rest = skip_whitespace(rest)?; // `\s+` after `->`
    let trigger = if rest.starts_with("single") {
        "single"
    } else if rest.starts_with("double") {
        "double"
    } else if rest.starts_with("hold") {
        "hold"
    } else {
        return None;
    };

    Some(DeviceEvent::ButtonEvent {
        mode: mode.to_string(),
        index: index - 1,
        trigger: trigger.to_string(),
    })
}

/// `\s+`: at least one Python-whitespace character. Returns the remainder after
/// the run, or `None` when there is no whitespace at all.
fn skip_whitespace(text: &str) -> Option<&str> {
    let trimmed = text.trim_start();
    (trimmed.len() < text.len()).then_some(trimmed)
}

// ---------------------------------------------------------------------------
// Candidate port filtering
// ---------------------------------------------------------------------------

/// True when `name` looks like a port an OctoPad is attached to: `/dev/ttyUSB*`
/// or `/dev/ttyACM*` on Linux, `COM*` on Windows (mirrors
/// `SerialDaemonListener.list_available_ports` in both Python apps).
pub fn is_candidate_port(name: &str) -> bool {
    #[cfg(windows)]
    {
        is_com_port(name)
    }
    #[cfg(not(windows))]
    {
        name.starts_with("/dev/ttyUSB") || name.starts_with("/dev/ttyACM")
    }
}

/// Filter an enumeration down to OctoPad-style ports, sorted for a stable scan
/// order (Python sorts the Linux glob results).
pub fn candidate_ports(ports: Vec<String>) -> Vec<String> {
    let mut candidates: Vec<String> = ports
        .into_iter()
        .filter(|port| is_candidate_port(port))
        .collect();
    candidates.sort();
    candidates
}

/// True when `name` looks like a Windows COM port (`COM3`, case-insensitive).
pub fn is_com_port(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.len() > 3 && upper.starts_with("COM") && upper[3..].bytes().all(|b| b.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// Serial-port I/O (`serial` feature only)
// ---------------------------------------------------------------------------

#[cfg(feature = "serial")]
mod serial_io {
    use std::io::{self, Read, Write};
    use std::time::{Duration, Instant};

    use serialport::{ClearBuffer, ErrorKind as PortErrorKind, SerialPort};

    use super::{take_line, Hello, SerialError};

    /// Baud rate used by the firmware and both Python apps (mirrors
    /// `SerialDaemonListener.baudrate = 115200`).
    pub const BAUD_RATE: u32 = 115_200;

    /// Per-read timeout while waiting for the HELLO line (mirrors the Python
    /// probe's `readline(timeout=0.3)`). The overall budget is the caller's
    /// `timeout_ms`; a probe returns within `timeout_ms` plus at most one read
    /// slice.
    const READ_SLICE: Duration = Duration::from_millis(300);

    /// Settle time after opening — the board reboots on the DTR auto-reset and
    /// needs a moment before it answers (mirrors Python's `time.sleep(0.1)`).
    const SETTLE: Duration = Duration::from_millis(100);

    /// Identity request written to the pad (firmware: `WHO -> HELLO:...`).
    const WHO_REQUEST: &[u8] = b"WHO\n";

    /// Enumerate the serial ports currently attached to the system.
    ///
    /// Returns the port names/paths (e.g. `/dev/ttyACM0`, `COM3`). With no
    /// serial device attached the result is an empty `Vec` — never a panic
    /// (Review Focus Input 2). Only the enumeration itself can fail, and it
    /// does so as a structured `Err` message.
    pub fn list_ports() -> Result<Vec<String>, String> {
        let ports = serialport::available_ports().map_err(|err| err.to_string())?;
        Ok(port_names(ports))
    }

    /// Map enumeration results to plain port names. The pure mapping is exposed
    /// so the empty-OS case ("nothing attached → empty list, no panic") can be
    /// tested deterministically without hardware.
    pub fn port_names(ports: Vec<serialport::SerialPortInfo>) -> Vec<String> {
        ports.into_iter().map(|info| info.port_name).collect()
    }

    /// Enumerate and keep only ports an OctoPad can live on, sorted.
    pub fn list_candidate_ports() -> Result<Vec<String>, String> {
        Ok(super::candidate_ports(list_ports()?))
    }

    /// Minimal HELLO probe: open `port_path` at [`BAUD_RATE`], ask the pad to
    /// identify itself, and read until a `HELLO:OCTOPAD:...` line arrives or
    /// `timeout_ms` elapses. The port is released cleanly on every path (the
    /// handle is dropped before returning).
    ///
    /// Returns:
    /// - `Ok(Some(hello))` — an OctoPad answered within the timeout;
    /// - `Ok(None)` — the port does not exist (graceful skip — nothing
    ///   attached) *or* no HELLO arrived within `timeout_ms`;
    /// - `Err(SerialError::Busy | PermissionDenied | ...)` — structured
    ///   open/read failure, e.g. the legacy Python app already owns the port.
    pub fn probe_hello(port_path: &str, timeout_ms: u64) -> Result<Option<Hello>, SerialError> {
        let mut port = match serialport::new(port_path, BAUD_RATE)
            .timeout(READ_SLICE)
            .open()
        {
            Ok(port) => port,
            Err(err) => {
                return match classify_port_failure(&format!("open {port_path}"), &err) {
                    // Nothing attached / path does not exist → skip, do not
                    // fail (the "USB serial not present" case).
                    SerialError::NotFound(_) => Ok(None),
                    structured => Err(structured),
                };
            }
        };

        let probe = read_hello(&mut port, timeout_ms);

        // Close cleanly on every path: dropping the port clears TIOCEXCL and
        // closes the fd (releasing the exclusive lock for the next client).
        drop(port);

        probe.map_err(|err| classify_port_failure(&format!("probe {port_path}"), &err))
    }

    /// Read until a `HELLO:OCTOPAD:...` line arrives or `timeout_ms` elapses.
    fn read_hello(
        port: &mut Box<dyn SerialPort>,
        timeout_ms: u64,
    ) -> Result<Option<Hello>, serialport::Error> {
        std::thread::sleep(SETTLE);
        // Stale input is harmless, but flushing keeps the buffer clean.
        let _ = port.clear(ClearBuffer::Input);

        port.write_all(WHO_REQUEST)?;

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut pending: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 256];

        loop {
            if Instant::now() >= deadline {
                return Ok(None); // timeout: no pad answered
            }
            match port.read(&mut chunk) {
                Ok(0) => {
                    // Defensive: never busy-spin if the platform returns an
                    // empty read without blocking.
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(n) => {
                    pending.extend_from_slice(&chunk[..n]);
                    while let Some(line) = take_line(&mut pending) {
                        if let Some(hello) = super::parse_hello(&line) {
                            return Ok(Some(hello));
                        }
                    }
                }
                // Read slice timed out — keep waiting until the overall
                // deadline (mirrors Python's empty-readline `continue`).
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(err) => return Err(err.into()),
            }
        }
    }

    /// Map a `serialport` failure to a structured [`SerialError`].
    ///
    /// The description wording matters because the crate's kind mapping is
    /// lossy on some platforms: Windows maps `ERROR_ACCESS_DENIED` to
    /// `NoDevice` with an "Access is denied." description (and
    /// `ERROR_SHARING_VIOLATION` to a plain `Io(Other)` whose message says
    /// "being used by another process"), while Linux maps `EBUSY`/flock
    /// contention to `NoDevice` with a "…busy…"/"…exclusive lock…"
    /// description.
    pub fn classify_port_failure(op: &str, err: &serialport::Error) -> SerialError {
        let reason = format!("{op}: {err}");
        let lower = reason.to_ascii_lowercase();

        if matches!(
            err.kind(),
            PortErrorKind::Io(io::ErrorKind::PermissionDenied)
        ) || lower.contains("permission denied")
            || lower.contains("access is denied")
        {
            return SerialError::PermissionDenied(reason);
        }
        if lower.contains("busy")
            || lower.contains("in use")
            || lower.contains("in-use")
            || lower.contains("sharing violation")
            || lower.contains("being used by another process")
            || lower.contains("exclusive lock")
        {
            return SerialError::Busy(reason);
        }
        if matches!(
            err.kind(),
            PortErrorKind::Io(io::ErrorKind::NotFound) | PortErrorKind::NoDevice
        ) || lower.contains("no such file")
            || lower.contains("cannot find")
            || lower.contains("not found")
        {
            return SerialError::NotFound(reason);
        }
        SerialError::Other(reason)
    }
}

#[cfg(feature = "serial")]
pub use serial_io::{
    classify_port_failure, list_candidate_ports, list_ports, port_names, probe_hello, BAUD_RATE,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn take_line_splits_lf_crlf_and_keeps_partial_tail() {
        let mut buf = b"hello\r\nworld\npar".to_vec();
        assert_eq!(take_line(&mut buf), Some("hello".to_string()));
        assert_eq!(take_line(&mut buf), Some("world".to_string()));
        assert_eq!(take_line(&mut buf), None); // no terminator yet
        assert_eq!(buf, b"par"); // partial line stays pending
    }

    #[test]
    fn candidate_ports_filters_to_pad_style_ports_and_sorts() {
        let picked = candidate_ports(vec![
            "/dev/ttyS0".to_string(),
            "/dev/ttyUSB0".to_string(),
            "/dev/ttyACM0".to_string(),
            "COM3".to_string(),
        ]);
        #[cfg(not(target_os = "windows"))]
        assert_eq!(picked, vec!["/dev/ttyACM0", "/dev/ttyUSB0"]);
        #[cfg(target_os = "windows")]
        assert_eq!(picked, vec!["COM3"]);
    }

    #[test]
    fn parse_event_line_returns_the_structured_button_event() {
        assert_eq!(
            parse_event_line("[HA] Button 1 -> HOLD"),
            Some(DeviceEvent::ButtonEvent {
                mode: "ha".to_string(),
                index: 0,
                trigger: "hold".to_string(),
            })
        );
    }

    #[test]
    fn empty_port_list_maps_to_empty_vec_without_panicking() {
        #[cfg(feature = "serial")]
        assert!(port_names(Vec::new()).is_empty());
    }

    #[test]
    fn serial_errors_display_without_panicking() {
        assert!(SerialError::Busy("x".into()).to_string().contains("busy"));
        assert!(!SerialError::PermissionDenied("x".into())
            .to_string()
            .is_empty());
        assert!(!SerialError::NotFound("x".into()).to_string().is_empty());
        assert!(!SerialError::Other("x".into()).to_string().is_empty());
    }

    #[cfg(feature = "serial")]
    mod serial {
        use super::super::*;
        use std::io;

        fn port_err(kind: serialport::ErrorKind, desc: &str) -> serialport::Error {
            serialport::Error::new(kind, desc)
        }

        #[test]
        fn busy_failures_classify_structurally() {
            // Linux: the terminal is already owned (the legacy Python app holds
            // the port).
            assert!(matches!(
                classify_port_failure(
                    "open /dev/ttyACM0",
                    &port_err(serialport::ErrorKind::NoDevice, "Device or resource busy")
                ),
                SerialError::Busy(_)
            ));
            // Linux: exclusive flock held by another client.
            assert!(matches!(
                classify_port_failure(
                    "open /dev/ttyACM0",
                    &port_err(
                        serialport::ErrorKind::NoDevice,
                        "Unable to acquire exclusive lock on serial port"
                    )
                ),
                SerialError::Busy(_)
            ));
            // Windows: ERROR_SHARING_VIOLATION (share_mode = 0).
            assert!(matches!(
                classify_port_failure(
                    "open COM3",
                    &port_err(
                        serialport::ErrorKind::Io(io::ErrorKind::Other),
                        "The process cannot access the file because it is being used by another process."
                    )
                ),
                SerialError::Busy(_)
            ));
        }

        #[test]
        fn permission_failures_classify_structurally() {
            assert!(matches!(
                classify_port_failure(
                    "open /dev/ttyACM0",
                    &port_err(
                        serialport::ErrorKind::Io(io::ErrorKind::PermissionDenied),
                        "Permission denied"
                    )
                ),
                SerialError::PermissionDenied(_)
            ));
            // Windows maps ERROR_ACCESS_DENIED to NoDevice with this wording.
            assert!(matches!(
                classify_port_failure(
                    "open COM3",
                    &port_err(serialport::ErrorKind::NoDevice, "Access is denied.")
                ),
                SerialError::PermissionDenied(_)
            ));
        }

        #[test]
        fn missing_port_classifies_as_not_found_for_graceful_skip() {
            assert!(matches!(
                classify_port_failure(
                    "open /dev/ttyNOPE",
                    &port_err(
                        serialport::ErrorKind::Io(io::ErrorKind::NotFound),
                        "No such file or directory"
                    )
                ),
                SerialError::NotFound(_)
            ));
            // Windows: ERROR_FILE_NOT_FOUND arrives as NoDevice + this wording.
            assert!(matches!(
                classify_port_failure(
                    "open C:\\nope",
                    &port_err(
                        serialport::ErrorKind::NoDevice,
                        "The system cannot find the file specified."
                    )
                ),
                SerialError::NotFound(_)
            ));
            // Anything else stays generic.
            assert!(matches!(
                classify_port_failure("probe", &port_err(serialport::ErrorKind::Unknown, "boom")),
                SerialError::Other(_)
            ));
        }
    }
}
