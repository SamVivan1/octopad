//! Task 3 tests: BLE discovery spike — pure matching logic (no hardware).
//!
//! These tests exercise only the classification logic, so they pass on
//! machines (and in CI) without a BLE adapter or an OctoPad nearby. The
//! hardware path is `#[ignore]`d at the bottom (Review Focus Input 5: the pad
//! must be accepted by service UUID even when its advertised name differs).

#![cfg(feature = "ble")]

use octopad_core::ble_spike::{classify_device, AdvertisedDevice};
use octopad_core::proto;

fn adv(name: Option<&str>, service_uuids: &[&str]) -> AdvertisedDevice {
    AdvertisedDevice {
        name: name.map(str::to_string),
        address: "aa:bb:cc:dd:ee:ff".to_string(),
        service_uuids: service_uuids.iter().map(|u| u.to_string()).collect(),
    }
}

#[test]
fn service_uuid_match_wins_even_when_name_differs() {
    // Review Focus Input 5: accepted by service UUID, advertised name is
    // something the user renamed the pad to.
    let found = classify_device(&adv(Some("My Custom Pad"), &[proto::SERVICE_UUID]))
        .expect("a device advertising our service UUID must be discovered");
    assert!(found.matched_by_uuid);
    assert_eq!(found.name, "My Custom Pad");
    assert_eq!(found.address, "aa:bb:cc:dd:ee:ff");
}

#[test]
fn legacy_nordic_uart_service_uuid_also_matches_by_uuid() {
    let found = classify_device(&adv(Some("OctoPad"), &[proto::LEGACY_SERVICE_UUID]))
        .expect("pre-rebrand firmware advertising the legacy NUS service must be discovered");
    assert!(found.matched_by_uuid);
}

#[test]
fn uuid_matching_is_case_insensitive() {
    // Advertisement data may vary in case (mirrors the Windows bridge, which
    // lowercases both sides before comparing).
    let found = classify_device(&adv(None, &["6F63746F-7061-4430-8001-000000000001"]))
        .expect("uppercase service UUID must match");
    assert!(found.matched_by_uuid);
    // Nameless UUID-matched device still discovered (name learned later via HELLO).
    assert_eq!(found.name, "");
}

#[test]
fn name_fallback_only_when_no_matching_uuid() {
    // No service UUIDs at all → default-name fallback applies
    // (mirrors Python: `name.casefold() == DEFAULT_DEVICE_NAME.casefold()`).
    let found = classify_device(&adv(Some("OctoPad"), &[]))
        .expect("default-named device without service UUIDs must be discovered");
    assert!(!found.matched_by_uuid, "name fallback is not a UUID match");

    // Non-matching UUIDs do not block the name fallback — Python only
    // short-circuits on our own service UUIDs, then falls back to the name.
    let found = classify_device(&adv(
        Some("OctoPad"),
        &["00001234-0000-1000-8000-00805f9b34fb"],
    ))
    .expect("default name must still fall back when no *matching* UUID is present");
    assert!(!found.matched_by_uuid);
}

#[test]
fn name_fallback_is_case_insensitive() {
    for name in ["OctoPad", "octopad", "OCTOPAD", "  OctoPad  "] {
        let found = classify_device(&adv(Some(name), &[])).unwrap_or_else(|| {
            panic!("{name:?} must match DEFAULT_DEVICE_NAME case-insensitively")
        });
        assert!(!found.matched_by_uuid);
        // Surrounding whitespace is stripped, mirroring Python's .strip().
        assert_eq!(found.name, name.trim());
    }
}

#[test]
fn unknown_devices_are_not_discovered() {
    // Unrelated device with a name and foreign service UUIDs.
    assert!(classify_device(&adv(
        Some("Random BLE Device"),
        &["00001234-0000-1000-8000-00805f9b34fb"]
    ))
    .is_none());
    // Anonymous device with nothing to go on.
    assert!(classify_device(&adv(None, &[])).is_none());
    // Empty/whitespace-only name is not the default name.
    assert!(classify_device(&adv(Some(""), &[])).is_none());
    assert!(classify_device(&adv(Some("   "), &[])).is_none());
    // A characteristic UUID must never count as a service match.
    assert!(classify_device(&adv(Some("Random BLE Device"), &[proto::TX_UUID])).is_none());
    assert!(classify_device(&adv(Some("Random BLE Device"), &[proto::LEGACY_RX_UUID])).is_none());
}

/// Hardware smoke test: discovery must never panic, only return structured
/// results/errors (Review Focus Inputs 1 and 2 analog for BLE). Ignored by
/// default — no adapter in CI, no device guaranteed.
#[tokio::test]
#[ignore = "requires a BLE adapter (and an OctoPad for a non-empty result)"]
async fn discover_octopad_with_hardware_does_not_panic() {
    let found = octopad_core::ble_spike::discover_octopad(3_000)
        .await
        .expect("discovery must surface structured BleError, not panic");
    eprintln!("discovered devices: {found:?}");
}
