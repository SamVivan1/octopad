//! Task 4 tests: legacy config path verification + serial spike (enumeration,
//! HELLO parsing, minimal probe).
//!
//! Everything here is hardware-free so it passes on machines with nothing
//! attached (Review Focus Input 2) and without a config directory present
//! (Review Focus Input 3). The HELLO cases mirror `tests/test_parsers.py` —
//! the same inputs must yield the same results as the legacy Python apps.

#![cfg(feature = "serial")]

use octopad_core::config_paths::legacy_config_dir;
use octopad_core::serial_spike::{list_ports, parse_hello, port_names, probe_hello, SerialError};

/// Review Focus Input 3: the legacy config path must come from platform
/// defaults (`~/.config/octopad` on Linux, `%APPDATA%\OctoPad` on Windows) and
/// computing it must never touch — or crash on — a missing directory.
#[test]
fn legacy_config_dir_matches_platform_suffix() {
    let dir = legacy_config_dir();
    assert!(
        !dir.as_os_str().is_empty(),
        "legacy_config_dir() must never be empty"
    );

    #[cfg(not(target_os = "windows"))]
    assert!(
        dir.ends_with(".config/octopad"),
        "expected ~/.config/octopad, got {dir:?}"
    );

    #[cfg(target_os = "windows")]
    assert!(
        dir.ends_with("OctoPad"),
        "expected %APPDATA%\\OctoPad, got {dir:?}"
    );
}

/// Review Focus Input 2: enumerating with no USB serial device attached must
/// return an empty list and must not panic. The empty-OS case is exercised
/// deterministically through the pure name mapping; the live enumeration below
/// only asserts that it reports a structured result instead of panicking.
#[test]
fn empty_port_enumeration_yields_empty_list_without_panicking() {
    let ports = port_names(Vec::new());
    assert!(ports.is_empty());
    assert_eq!(ports, Vec::<String>::new());
}

/// Live smoke check: `available_ports()` must answer with a structured result
/// (whatever the OS reports), never a panic.
#[test]
fn list_ports_enumerates_without_panicking() {
    let ports = list_ports().expect("serialport::available_ports() must not fail");
    eprintln!("serial ports found: {ports:?}");
    assert!(ports.iter().all(|name| !name.trim().is_empty()));
}

/// A port that does not exist is a graceful skip (`Ok(None)`), not an error —
/// the analog of "USB serial not present" for the probe (Review Focus Input
/// 2), and the reason the spike can be run with nothing attached.
#[test]
fn probe_hello_skips_gracefully_when_port_is_not_found() {
    let missing =
        std::env::temp_dir().join(format!("octopad-m0-no-such-port-{}", std::process::id()));
    let result = probe_hello(&missing.to_string_lossy(), 200)
        .expect("a missing port must be skipped, never surface an error");
    assert_eq!(result, None);
}

// --- HELLO parsing: same cases as tests/test_parsers.py ---------------------

#[test]
fn parse_hello_extracts_name_and_version() {
    let hello = parse_hello("HELLO:OCTOPAD:My Pad:v2.6")
        .expect("identity line from the firmware must parse");
    assert_eq!(hello.name, "My Pad");
    assert_eq!(hello.version, "2.6");
}

#[test]
fn parse_hello_without_version() {
    let hello =
        parse_hello("HELLO:OCTOPAD:My Pad").expect("identity line without a version must parse");
    assert_eq!(hello.name, "My Pad");
    assert_eq!(hello.version, "");
}

#[test]
fn parse_hello_rejects_other_brand() {
    assert!(parse_hello("HELLO:OTHER:Pad:v1").is_none());
    assert!(parse_hello("HELLO:OTHER:Pad").is_none());
    // Not an identity line at all.
    assert!(parse_hello("[DESKTOP] Button 3 -> DOUBLE").is_none());
    assert!(parse_hello("").is_none());
}

#[test]
fn parse_hello_defaults_when_unnamed() {
    let hello =
        parse_hello("HELLO:OCTOPAD::v2.6").expect("unnamed pad must fall back to the default name");
    assert_eq!(hello.name, "OctoPad");
    assert_eq!(hello.version, "2.6");
}

#[test]
fn parse_hello_is_case_insensitive_and_finds_line_within_noise() {
    // Python uses `re.search(..., re.IGNORECASE)`: the brand tag and the
    // HELLO keyword are matched case-insensitively, anywhere in the line.
    let hello = parse_hello("[BOOT] hello:octopad:Pad:v1.2")
        .expect("lowercase HELLO inside surrounding noise must parse");
    assert_eq!(hello.name, "Pad");
    assert_eq!(hello.version, "1.2");

    // A version must start with a digit; `:vX` is not one (Python's
    // `v?([0-9]...)` fails to match and the whole group stays empty).
    let hello = parse_hello("HELLO:OCTOPAD:Pad:garbage")
        .expect("line must still parse when the version group does not fit");
    assert_eq!(hello.name, "Pad");
    assert_eq!(hello.version, "");
}

// --- Structured errors ------------------------------------------------------

/// Busy (legacy Python app owns the port — Review Focus Input 1) and
/// permission failures must be distinguishable machine-readably, not just as
/// prose: the Tauri command layer serializes these straight to the frontend.
#[test]
fn serial_errors_serialize_as_structured_kinds() {
    let busy = SerialError::Busy("probe /dev/ttyACM0: Device or resource busy".into());
    let permission = SerialError::PermissionDenied("probe /dev/ttyACM0: Permission denied".into());

    let busy_value = serde_json::to_value(&busy).expect("SerialError must serialize");
    let permission_value = serde_json::to_value(&permission).expect("SerialError must serialize");
    assert!(busy_value.get("Busy").is_some(), "got {busy_value}");
    assert!(
        permission_value.get("PermissionDenied").is_some(),
        "got {permission_value}"
    );

    // Display never panics and carries the underlying reason.
    let busy_text = busy.to_string().to_ascii_lowercase();
    assert!(busy_text.contains("busy"), "got {busy_text}");
    let permission_text = permission.to_string().to_ascii_lowercase();
    assert!(
        permission_text.contains("permission"),
        "got {permission_text}"
    );
}

/// Hardware smoke test (ignored by default): enumeration + probe must produce
/// only structured results — never a panic — even with a device attached.
/// Running this deliberately: `cargo test -p octopad-core --features serial
/// --test serial_config -- --ignored`.
#[test]
#[ignore = "requires an attached OctoPad serial port (probe writes WHO\\n)"]
fn probe_hello_with_hardware_never_panics() {
    let ports = list_ports().expect("enumeration must not fail");
    // Only probe ports a pad can realistically be: the ESP32 shows up as
    // ttyACM/ttyUSB on Linux, a COM port on Windows.
    let candidates: Vec<String> = ports
        .into_iter()
        .filter(|name| {
            name.contains("ttyACM") || name.contains("ttyUSB") || name.starts_with("COM")
        })
        .collect();
    assert!(
        !candidates.is_empty(),
        "attach an OctoPad via USB (or fix the port filter)"
    );
    for port in candidates {
        match probe_hello(&port, 2_000) {
            Ok(hello) => eprintln!("probe {port}: {hello:?}"),
            Err(err) => eprintln!("probe {port}: structured error: {err} ({err:?})"),
        }
    }
}
