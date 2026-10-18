//! Thin re-export shim for the M0 BLE spike (Task 3).
//!
//! The pure classification logic moved to `device::protocol` and the
//! `btleplug` plumbing to `device::ble_transport`, so the transport and the
//! spike share one implementation. The `octopad_core::ble_spike::*` paths stay
//! working for the existing tests and callers (mirroring `serial_spike`).

pub use crate::device::ble_transport::{connect_and_read_tx, discover_octopad, ConnectProbe};
pub use crate::device::protocol::{
    classify_all, classify_ble_failure, classify_device, is_default_device_name,
    is_rx_characteristic, is_tx_characteristic, AdvertisedDevice, BleError, Discovered,
    DEFAULT_DEVICE_NAME,
};
