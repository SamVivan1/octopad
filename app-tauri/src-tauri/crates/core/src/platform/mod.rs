//! Platform-specific utilities.

/// Enumerate the serial ports currently attached to the system.
///
/// Returns the port names/paths (e.g. `/dev/ttyACM0`, `COM3`). When no serial
/// device is attached the result is an empty `Vec` — never a panic. The
/// enumeration itself is the only failure mode (reported as a message).
///
/// Available only with the `serial` feature.
#[cfg(feature = "serial")]
pub fn list_ports() -> Result<Vec<String>, String> {
    let ports = serialport::available_ports().map_err(|err| err.to_string())?;
    Ok(ports.into_iter().map(|info| info.port_name).collect())
}
