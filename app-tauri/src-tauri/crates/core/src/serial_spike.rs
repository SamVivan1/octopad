//! Serial enumeration + minimal HELLO probe (`serial` feature only).
//!
//! Mirrors the USB-serial half of the legacy Python app
//! (`linux-app/serial_listener.py::_probe_port`, `parse_hello`) so the M0
//! spike behaves exactly like the apps it replaces:
//!
//! - enumerate ports ([`list_ports`]) — a system with nothing attached yields
//!   an empty list and never a panic (Review Focus Input 2);
//! - [`probe_hello`] opens the port at 115200 8N1, settles 100 ms (Arduino
//!   auto-reset on DTR), flushes stale input, asks `WHO\n`, and reads until a
//!   `HELLO:OCTOPAD:<name>:v<version>` line arrives or the timeout elapses;
//! - the port is always released cleanly (RAII: dropping the handle clears
//!   `TIOCEXCL` and closes the fd — `serialport` has no explicit `close()`);
//! - failures come back as structured [`SerialError`]s: **busy** (the legacy
//!   Python app already owns the port — Review Focus Input 1) and
//!   **permission denied** are machine-readable kinds, while a port that does
//!   not exist is a graceful skip (`Ok(None)`), not an error.
//!
//! The HELLO parser mirrors `parse_hello` in `tests/test_parsers.py`: same
//! pattern (`HELLO:<brand>:<name>[:v<version>]`, case-insensitive), same brand
//! gate (`OCTOPAD`), same default name for an unnamed pad.

use std::fmt;
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use serialport::{ClearBuffer, ErrorKind as PortErrorKind, SerialPort};

/// Baud rate used by the firmware and both Python apps (mirrors
/// `SerialDaemonListener.baudrate = 115200`).
pub const BAUD_RATE: u32 = 115_200;

/// Brand tag inside the identity line, mirroring `BRAND_TAG` in the Python
/// listeners. Only lines carrying this tag are genuine OctoPad HELLOs.
pub const BRAND_TAG: &str = "OCTOPAD";

/// Default name when the pad reports an empty one (Python:
/// `name.strip() or "OctoPad"`).
pub const DEFAULT_DEVICE_NAME: &str = "OctoPad";

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

/// Parsed firmware identity line: `HELLO:OCTOPAD:<name>:v<version>`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Hello {
    /// User-configured device name (defaults to [`DEFAULT_DEVICE_NAME`]).
    pub name: String,
    /// Firmware version, empty when the line carries none.
    pub version: String,
}

/// Structured serial errors — the spike never panics on hardware failures and
/// callers can distinguish "busy" (port owned by another client, e.g. the
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

/// Enumerate the serial ports currently attached to the system.
///
/// Returns the port names/paths (e.g. `/dev/ttyACM0`, `COM3`). With no serial
/// device attached the result is an empty `Vec` — never a panic (Review Focus
/// Input 2). Only the enumeration itself can fail, and it does so as a
/// structured `Err` message.
pub fn list_ports() -> Result<Vec<String>, String> {
    let ports = serialport::available_ports().map_err(|err| err.to_string())?;
    Ok(port_names(ports))
}

/// Map enumeration results to plain port names. The pure mapping is exposed so
/// the empty-OS case ("nothing attached → empty list, no panic") can be tested
/// deterministically without hardware.
pub fn port_names(ports: Vec<serialport::SerialPortInfo>) -> Vec<String> {
    ports.into_iter().map(|info| info.port_name).collect()
}

/// Minimal HELLO probe (spike scope): open `port_path` at [`BAUD_RATE`], ask
/// the pad to identify itself, and read until a `HELLO:OCTOPAD:...` line
/// arrives or `timeout_ms` elapses. The port is released cleanly on every
/// path (the handle is dropped before returning).
///
/// Returns:
/// - `Ok(Some(hello))` — an OctoPad answered within the timeout;
/// - `Ok(None)` — the port does not exist (graceful skip — nothing attached)
///   *or* no HELLO arrived within `timeout_ms`;
/// - `Err(SerialError::Busy | PermissionDenied | ...)` — structured open/read
///   failure, e.g. the legacy Python app already owns the port.
pub fn probe_hello(port_path: &str, timeout_ms: u64) -> Result<Option<Hello>, SerialError> {
    let mut port = match serialport::new(port_path, BAUD_RATE)
        .timeout(READ_SLICE)
        .open()
    {
        Ok(port) => port,
        Err(err) => {
            return match classify_serial_failure(&format!("open {port_path}"), &err) {
                // Nothing attached / path does not exist → skip, do not fail
                // (Review Focus Input 2 analog for the probe).
                SerialError::NotFound(_) => Ok(None),
                structured => Err(structured),
            };
        }
    };

    let probe = read_hello(&mut port, timeout_ms);

    // Close cleanly on every path: dropping the port clears TIOCEXCL and
    // closes the fd (releasing the exclusive lock for the next client).
    drop(port);

    probe.map_err(|err| classify_serial_failure(&format!("probe {port_path}"), &err))
}

/// Ask the open port for its identity and read until [`parse_hello`] matches
/// or the budget runs out. I/O errors surface as [`serialport::Error`] for
/// classification by the caller.
fn read_hello(
    port: &mut Box<dyn SerialPort>,
    timeout_ms: u64,
) -> serialport::Result<Option<Hello>> {
    // Mirror the Python probe: settle after the auto-reset, flush whatever
    // the reboot printed, then request the identity explicitly.
    std::thread::sleep(SETTLE);
    if let Err(err) = port.clear(ClearBuffer::Input) {
        // Stale bytes are harmless: WHO re-requests a fresh identity line.
        tracing::warn!(error = %err, "clearing serial input buffer failed");
    }
    port.write_all(WHO_REQUEST)
        .map_err(serialport::Error::from)?;

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
                    if let Some(hello) = parse_hello(&line) {
                        return Ok(Some(hello));
                    }
                }
            }
            // Read slice timed out — keep waiting until the overall deadline
            // (mirrors Python's empty-readline `continue`).
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(err) => return Err(serialport::Error::from(err)),
        }
    }
}

/// Pop one complete line (`\n`-terminated, optional trailing `\r`) from
/// `pending`, mirroring the Python probe's `readline()` loop. An incomplete
/// tail stays buffered.
fn take_line(pending: &mut Vec<u8>) -> Option<String> {
    let idx = pending.iter().position(|&b| b == b'\n')?;
    let line = pending.drain(..=idx).collect::<Vec<u8>>();
    let mut text = String::from_utf8_lossy(&line[..idx]).into_owned(); // drop the '\n'
    if text.ends_with('\r') {
        text.pop();
    }
    Some(text)
}

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

/// Map a `serialport` failure to a structured [`SerialError`].
///
/// The description wording matters because the crate's kind mapping is lossy
/// on some platforms: Windows maps `ERROR_ACCESS_DENIED` to `NoDevice` with an
/// "Access is denied." description (and `ERROR_SHARING_VIOLATION` to a plain
/// `Io(Other)` whose message says "being used by another process"), while
/// Linux maps `EBUSY`/flock contention to `NoDevice` with a "…busy…"/
/// "…exclusive lock…" description.
fn classify_serial_failure(op: &str, err: &serialport::Error) -> SerialError {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn port_err(kind: PortErrorKind, desc: &str) -> serialport::Error {
        serialport::Error::new(kind, desc)
    }

    #[test]
    fn busy_failures_classify_structurally() {
        // Linux: the terminal is already owned (Review Focus Input 1 — the
        // legacy Python app holds the port).
        assert!(matches!(
            classify_serial_failure(
                "open /dev/ttyACM0",
                &port_err(PortErrorKind::NoDevice, "Device or resource busy")
            ),
            SerialError::Busy(_)
        ));
        // Linux: exclusive flock held by another client.
        assert!(matches!(
            classify_serial_failure(
                "open /dev/ttyACM0",
                &port_err(
                    PortErrorKind::NoDevice,
                    "Unable to acquire exclusive lock on serial port"
                )
            ),
            SerialError::Busy(_)
        ));
        // Windows: ERROR_SHARING_VIOLATION (share_mode = 0).
        assert!(matches!(
            classify_serial_failure(
                "open COM3",
                &port_err(
                    PortErrorKind::Io(io::ErrorKind::Other),
                    "The process cannot access the file because it is being used by another process."
                )
            ),
            SerialError::Busy(_)
        ));
    }

    #[test]
    fn permission_failures_classify_structurally() {
        assert!(matches!(
            classify_serial_failure(
                "open /dev/ttyACM0",
                &port_err(
                    PortErrorKind::Io(io::ErrorKind::PermissionDenied),
                    "Permission denied"
                )
            ),
            SerialError::PermissionDenied(_)
        ));
        // Windows maps ERROR_ACCESS_DENIED to NoDevice with this wording.
        assert!(matches!(
            classify_serial_failure(
                "open COM3",
                &port_err(PortErrorKind::NoDevice, "Access is denied.")
            ),
            SerialError::PermissionDenied(_)
        ));
    }

    #[test]
    fn missing_port_classifies_as_not_found_for_graceful_skip() {
        assert!(matches!(
            classify_serial_failure(
                "open /dev/ttyNOPE",
                &port_err(
                    PortErrorKind::Io(io::ErrorKind::NotFound),
                    "No such file or directory"
                )
            ),
            SerialError::NotFound(_)
        ));
        // Windows: ERROR_FILE_NOT_FOUND arrives as NoDevice + this wording.
        assert!(matches!(
            classify_serial_failure(
                "open C:\\nope",
                &port_err(
                    PortErrorKind::NoDevice,
                    "The system cannot find the file specified."
                )
            ),
            SerialError::NotFound(_)
        ));
        // Anything else stays generic.
        assert!(matches!(
            classify_serial_failure("probe", &port_err(PortErrorKind::Unknown, "boom")),
            SerialError::Other(_)
        ));
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

    #[test]
    fn take_line_splits_lf_crlf_and_keeps_partial_tail() {
        let mut buf = b"hello\r\nworld\npar".to_vec();
        assert_eq!(take_line(&mut buf), Some("hello".to_string()));
        assert_eq!(take_line(&mut buf), Some("world".to_string()));
        assert_eq!(take_line(&mut buf), None); // no terminator yet
        assert_eq!(buf, b"par"); // partial line stays pending
    }

    #[test]
    fn empty_port_list_maps_to_empty_vec_without_panicking() {
        assert!(port_names(Vec::new()).is_empty());
    }
}
