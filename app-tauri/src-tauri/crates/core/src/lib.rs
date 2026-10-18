pub mod config_paths;
pub mod platform;
pub mod proto;

#[cfg(feature = "ble")]
pub mod ble_spike;

#[cfg(feature = "serial")]
pub mod serial_spike;

#[cfg(feature = "serial")]
pub use platform::list_ports;

pub fn version() -> &'static str {
    "0.1.0"
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_non_empty() {
        assert!(!super::version().is_empty());
    }
}
