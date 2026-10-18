//! Device protocol constants.
//!
//! Exact mirrors of the UUIDs in the Python apps (`linux-app/ble_bridge.py`,
//! `windows-app/ble_bridge.py`) and the Arduino firmware (`octopad.ino`).
//! The legacy Nordic-UART UUIDs come from pre-rebrand firmware and must keep
//! being accepted during discovery.

/// OctoPad primary service UUID (advertised by the pad).
pub const SERVICE_UUID: &str = "6f63746f-7061-4430-8001-000000000001";
/// TX characteristic (device → host): notify/read.
pub const TX_UUID: &str = "6f63746f-7061-4430-8001-000000000002";
/// RX characteristic (host → device): write.
pub const RX_UUID: &str = "6f63746f-7061-4430-8001-000000000003";
/// Legacy Nordic-UART service UUID, accepted for discovery.
pub const LEGACY_SERVICE_UUID: &str = "6e400001-b5a3-f393-e0a9-e50e24dcca9e";
/// Legacy Nordic-UART TX characteristic.
pub const LEGACY_TX_UUID: &str = "6e400003-b5a3-f393-e0a9-e50e24dcca9e";
/// Legacy Nordic-UART RX characteristic.
pub const LEGACY_RX_UUID: &str = "6e400002-b5a3-f393-e0a9-e50e24dcca9e";

/// True when `uuid` identifies an OctoPad device: the primary service or the
/// legacy Nordic-UART service.
///
/// Comparison is case-insensitive, mirroring the Python bridges, which match
/// `SERVICE_UUID in uuids or LEGACY_SERVICE_UUID in uuids` (Windows bridge
/// lowercases both sides). Characteristic UUIDs never match.
pub fn is_our_service(uuid: &str) -> bool {
    let uuid = uuid.to_ascii_lowercase();
    uuid == SERVICE_UUID || uuid == LEGACY_SERVICE_UUID
}
