//! Task 2 integration tests: protocol UUID constants, legacy config paths and
//! (feature-gated) serial port enumeration.
//!
//! The UUID strings must match the Python apps exactly — see
//! `linux-app/ble_bridge.py` and `windows-app/ble_bridge.py` — and are also
//! cross-checked against the firmware (`octopad.ino`).

use octopad_core::config_paths::legacy_config_dir;
use octopad_core::proto;

#[test]
fn primary_uuids_match_python_apps() {
    assert_eq!(proto::SERVICE_UUID, "6f63746f-7061-4430-8001-000000000001");
    assert_eq!(proto::TX_UUID, "6f63746f-7061-4430-8001-000000000002");
    assert_eq!(proto::RX_UUID, "6f63746f-7061-4430-8001-000000000003");
}

#[test]
fn legacy_nordic_uart_uuids_match_python_apps() {
    assert_eq!(
        proto::LEGACY_SERVICE_UUID,
        "6e400001-b5a3-f393-e0a9-e50e24dcca9e"
    );
    assert_eq!(
        proto::LEGACY_TX_UUID,
        "6e400003-b5a3-f393-e0a9-e50e24dcca9e"
    );
    assert_eq!(
        proto::LEGACY_RX_UUID,
        "6e400002-b5a3-f393-e0a9-e50e24dcca9e"
    );
}

#[test]
fn is_our_service_accepts_primary_and_legacy_service_only() {
    // The pad is accepted by service UUID (mirrors Python: `SERVICE_UUID in
    // uuids or LEGACY_SERVICE_UUID in uuids`), regardless of advertised name.
    assert!(proto::is_our_service(proto::SERVICE_UUID));
    assert!(proto::is_our_service(proto::LEGACY_SERVICE_UUID));
    // Advertisement data may vary in case; comparison is case-insensitive
    // (mirrors the Windows bridge, which lowercases both sides).
    assert!(proto::is_our_service(
        "6F63746F-7061-4430-8001-000000000001"
    ));
    // Characteristic UUIDs are never a service match.
    assert!(!proto::is_our_service(proto::TX_UUID));
    assert!(!proto::is_our_service(proto::RX_UUID));
    assert!(!proto::is_our_service(proto::LEGACY_TX_UUID));
    assert!(!proto::is_our_service(proto::LEGACY_RX_UUID));
    assert!(!proto::is_our_service(""));
    assert!(!proto::is_our_service("not-a-uuid"));
}

#[test]
fn legacy_config_dir_is_non_empty_and_matches_platform_suffix() {
    // Missing directory must not crash: this only computes a path.
    let dir = legacy_config_dir();
    assert!(
        !dir.as_os_str().is_empty(),
        "legacy_config_dir() must never be empty"
    );

    #[cfg(target_os = "windows")]
    assert!(
        dir.ends_with("OctoPad"),
        "expected %APPDATA%\\OctoPad, got {dir:?}"
    );

    #[cfg(not(target_os = "windows"))]
    assert!(
        dir.ends_with(".config/octopad"),
        "expected ~/.config/octopad, got {dir:?}"
    );
}

// Enumeration must never panic: with no USB serial device attached it returns
// an empty list (Review Focus Input 2).
#[cfg(feature = "serial")]
#[test]
fn list_ports_enumerates_without_panicking() {
    let ports = octopad_core::list_ports()
        .expect("serialport::available_ports() must not error during enumeration");
    eprintln!("serial ports found: {ports:?}");
}
