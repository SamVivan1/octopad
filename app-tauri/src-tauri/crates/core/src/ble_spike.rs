//! BLE discovery + minimal-connect spike (`ble` feature only).
//!
//! Mirrors the matching rules of the Python bridges (`linux-app/ble_bridge.py`,
//! `windows-app/ble_bridge.py`): a device is an OctoPad when it advertises the
//! primary service UUID or the legacy Nordic-UART service UUID; the default
//! name `OctoPad` is only a fallback when no matching service UUID was
//! advertised (Review Focus Input 5 — the pad is accepted by UUID whatever it
//! is called).
//!
//! The pure classification logic (`classify_device`/`classify_all`) is
//! hardware-free and unit-tested; the `btleplug`-driven I/O paths are async
//! and deliberately minimal (spike scope): scan with timeout, one connect, one
//! TX read (for the early `HELLO:OCTOPAD:<name>:v<ver>` line), clean
//! disconnect. No full event stream, no reconnect logic.

use std::fmt;
use std::time::Duration;

use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, Peripheral};

use crate::proto;

/// Fallback advertised name, mirroring `DEFAULT_DEVICE_NAME` in the Python
/// bridges. Only consulted when no matching service UUID was advertised.
pub const DEFAULT_DEVICE_NAME: &str = "OctoPad";

/// One device as seen during a scan, reduced to what classification needs.
#[derive(Debug, Clone)]
pub struct AdvertisedDevice {
    /// Advertised local name, if any.
    pub name: Option<String>,
    /// Address in string form (platform display form, e.g. `aa:bb:cc:dd:ee:ff`).
    pub address: String,
    /// Advertised service UUIDs, as strings (case as delivered by the OS).
    pub service_uuids: Vec<String>,
}

/// An OctoPad candidate produced by discovery/classification.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Discovered {
    /// Advertised name (may be empty when the pad was matched by UUID only;
    /// the user-configured name is learned later from the HELLO line).
    pub name: String,
    /// Device address in string form.
    pub address: String,
    /// `true` when accepted via advertised service UUID (strong signal),
    /// `false` when accepted via the default-name fallback only.
    pub matched_by_uuid: bool,
}

/// Result of the minimal connect probe (spike scope).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectProbe {
    /// The device that was connected to.
    pub discovered: Discovered,
    /// Raw bytes read from the TX characteristic, if the plain read succeeded.
    /// Pads send `HELLO:OCTOPAD:<name>:v<ver>` early; notify-only setups may
    /// reject a plain read, which is *not* an error for the spike.
    pub tx_read: Option<String>,
}

/// Structured BLE errors — the spike never panics on hardware failures and
/// callers can distinguish "busy" (device already owned by another client,
/// e.g. the running Python BLE bridge) from other failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BleError {
    /// No Bluetooth adapter is present or available.
    NoAdapter,
    /// The adapter/device reports the resource busy or already in use.
    Busy(String),
    /// Scan/connect/GATT failure, with the underlying reason.
    Other(String),
}

impl fmt::Display for BleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BleError::NoAdapter => write!(f, "no Bluetooth adapter available"),
            BleError::Busy(msg) => write!(f, "Bluetooth device busy: {msg}"),
            BleError::Other(msg) => write!(f, "Bluetooth error: {msg}"),
        }
    }
}

impl std::error::Error for BleError {}

/// True when `name` equals [`DEFAULT_DEVICE_NAME`], trimmed and
/// case-insensitively (mirrors Python:
/// `(Name or Alias or "").strip().casefold() == DEFAULT_DEVICE_NAME.casefold()`).
pub fn is_default_device_name(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(DEFAULT_DEVICE_NAME)
}

/// Classify one advertised device: service-UUID match first
/// (`matched_by_uuid = true`), default-name fallback only when no advertised
/// UUID matched. Returns `None` for anything that is not an OctoPad.
pub fn classify_device(adv: &AdvertisedDevice) -> Option<Discovered> {
    let matched_by_uuid = adv
        .service_uuids
        .iter()
        .any(|uuid| proto::is_our_service(uuid));
    let name = adv.name.as_deref().unwrap_or("").trim();
    if !matched_by_uuid && !is_default_device_name(name) {
        return None;
    }
    Some(Discovered {
        name: name.to_string(),
        address: adv.address.clone(),
        matched_by_uuid,
    })
}

/// Classify a whole scan window, preserving scan order.
pub fn classify_all(devices: &[AdvertisedDevice]) -> Vec<Discovered> {
    devices.iter().filter_map(classify_device).collect()
}

/// Discover OctoPad devices: scan for `timeout_ms`, then classify everything
/// heard. Structured [`BleError`] on failure — never a panic.
pub async fn discover_octopad(timeout_ms: u64) -> Result<Vec<Discovered>, BleError> {
    let scanned = scan(timeout_ms).await?;
    Ok(scanned
        .iter()
        .filter_map(|(adv, _)| classify_device(adv))
        .collect())
}

/// Minimal connect spike: find the first OctoPad from a fresh scan (by
/// service UUID, then default-name fallback), connect, attempt one read of the
/// TX characteristic (device → host), then disconnect cleanly.
///
/// Returns `Ok(None)` when no adapter or no matching device is present (skip
/// gracefully). A busy adapter/GATT slot — e.g. the Python BLE bridge already
/// owns the connection — surfaces as [`BleError::Busy`].
pub async fn connect_and_read_tx(timeout_ms: u64) -> Result<Option<ConnectProbe>, BleError> {
    let scanned = match scan(timeout_ms).await {
        Ok(scanned) => scanned,
        Err(BleError::NoAdapter) => return Ok(None), // skip gracefully
        Err(err) => return Err(err),
    };
    let Some((adv, peri)) = scanned.into_iter().find(|(adv, _)| classify_device(adv).is_some())
    else {
        return Ok(None); // no OctoPad in range — skip gracefully
    };
    let discovered = classify_device(&adv).expect("device matched just above");

    peri.connect()
        .await
        .map_err(|err| classify_failure("connect", &err.to_string()))?;

    // One TX read for the early HELLO line; failures here are not fatal.
    let tx_read = match read_tx_once(&peri).await {
        Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Err(err) => {
            tracing::warn!(error = %err, "TX read failed (notify-only pads reject plain reads)");
            None
        }
    };

    // Disconnect cleanly; a failed disconnect must not hide the probe result.
    if let Err(err) = peri.disconnect().await {
        tracing::warn!(error = %err, "disconnect failed");
    }

    Ok(Some(ConnectProbe {
        discovered,
        tx_read,
    }))
}

/// Scan unfiltered for `timeout_ms` and collect everything advertised, paired
/// with its [`Peripheral`] for later connection. Unreadable entries are
/// skipped; the scan is always stopped before returning.
async fn scan(timeout_ms: u64) -> Result<Vec<(AdvertisedDevice, Peripheral)>, BleError> {
    let adapter = first_adapter().await?;
    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|err| classify_failure("start_scan", &err.to_string()))?;
    tokio::time::sleep(Duration::from_millis(timeout_ms)).await;

    let mut out = Vec::new();
    for peri in adapter
        .peripherals()
        .await
        .map_err(|err| classify_failure("peripherals", &err.to_string()))?
    {
        let address = peri.address().to_string();
        let Some(props) = peri.properties().await.ok().flatten() else {
            continue;
        };
        out.push((
            AdvertisedDevice {
                name: props.local_name.clone(),
                address,
                service_uuids: props.services.iter().map(|u| u.to_string()).collect(),
            },
            peri,
        ));
    }

    if let Err(err) = adapter.stop_scan().await {
        tracing::warn!(error = %err, "stop_scan failed");
    }
    Ok(out)
}

async fn first_adapter() -> Result<Adapter, BleError> {
    let manager = Manager::new()
        .await
        .map_err(|err| BleError::Other(format!("Bluetooth manager unavailable: {err}")))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|err| BleError::Other(format!("listing adapters failed: {err}")))?;
    adapters.into_iter().next().ok_or(BleError::NoAdapter)
}

/// Read the TX characteristic (primary or legacy Nordic-UART) once, after
/// service discovery. Mirrors the Python bridge, which reads TX once to fetch
/// the HELLO identity line.
async fn read_tx_once(peri: &Peripheral) -> Result<Vec<u8>, BleError> {
    peri.discover_services()
        .await
        .map_err(|err| classify_failure("discover_services", &err.to_string()))?;
    let tx = peri
        .characteristics()
        .into_iter()
        .find(|char| is_tx_characteristic(&char.uuid.to_string()))
        .ok_or_else(|| BleError::Other("TX characteristic not found on device".into()))?;
    peri.read(&tx)
        .await
        .map_err(|err| classify_failure("read TX", &err.to_string()))
}

/// True for the device → host TX characteristic of current or legacy firmware
/// (mirrors Python: `char in (TX_UUID, LEGACY_TX_UUID)`).
fn is_tx_characteristic(uuid: &str) -> bool {
    let uuid = uuid.to_ascii_lowercase();
    uuid == proto::TX_UUID || uuid == proto::LEGACY_TX_UUID
}

/// Map an operation failure to a structured error, flagging "device busy"
/// (adapter/GATT in use — e.g. the Python BLE bridge owns the connection).
fn classify_failure(op: &str, message: &str) -> BleError {
    let reason = format!("{op} failed: {message}");
    let lower = reason.to_ascii_lowercase();
    if lower.contains("busy")
        || lower.contains("in use")
        || lower.contains("in-use")
        || lower.contains("already connected")
        || lower.contains("already registered")
    {
        BleError::Busy(reason)
    } else {
        BleError::Other(reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tx_characteristic_recognized_for_current_and_legacy_firmware_only() {
        assert!(is_tx_characteristic(proto::TX_UUID));
        assert!(is_tx_characteristic(proto::LEGACY_TX_UUID));
        assert!(is_tx_characteristic("6F63746F-7061-4430-8001-000000000002"));
        // RX and service UUIDs are never the TX characteristic.
        assert!(!is_tx_characteristic(proto::RX_UUID));
        assert!(!is_tx_characteristic(proto::LEGACY_RX_UUID));
        assert!(!is_tx_characteristic(proto::SERVICE_UUID));
    }

    #[test]
    fn busy_failures_are_classified_structurally() {
        // Device already owned by another client (Review Focus Input 1).
        assert!(matches!(
            classify_failure("connect", "GATT server is busy"),
            BleError::Busy(_)
        ));
        assert!(matches!(
            classify_failure("connect", "Resource busy: device in use"),
            BleError::Busy(_)
        ));
        // Anything else is a generic error.
        assert!(matches!(
            classify_failure("connect", "protocol failure"),
            BleError::Other(_)
        ));
    }

    #[test]
    fn ble_errors_display_without_panicking() {
        assert!(!BleError::NoAdapter.to_string().is_empty());
        assert!(!BleError::Busy("x".into()).to_string().is_empty());
        assert!(!BleError::Other("x".into()).to_string().is_empty());
    }
}
