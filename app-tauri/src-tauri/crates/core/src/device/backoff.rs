//! Exponential reconnect backoff.
//!
//! Exact port of the BLE bridge's ladder (`linux-app/ble_bridge.py` and
//! `windows-app/ble_bridge.py`):
//!
//! ```python
//! _RECONNECT_S = 3.0
//! _RECONNECT_MAX_S = 15.0
//! delay = self._reconnect_delay
//! self._reconnect_delay = min(self._reconnect_delay * 2, _RECONNECT_MAX_S)
//! # and, on a successful connect: self._reconnect_delay = _RECONNECT_S
//! ```
//!
//! Pure and hardware-free: the transports own the actual waiting, this module
//! only decides how long the next wait is.

use std::time::Duration;

/// First reconnect delay (Python `_RECONNECT_S = 3.0`).
pub const BLE_INITIAL_DELAY: Duration = Duration::from_secs(3);

/// Cap for the doubling ladder (Python `_RECONNECT_MAX_S = 15.0`).
pub const BLE_MAX_DELAY: Duration = Duration::from_secs(15);

/// Exponential reconnect backoff: [`Backoff::next_delay`] returns the current
/// delay and doubles it, capped at `max`; [`Backoff::reset`] restores the
/// initial delay after a successful connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backoff {
    initial: Duration,
    max: Duration,
    next: Duration,
}

impl Backoff {
    /// A backoff that starts at `initial` and doubles up to `max`.
    pub const fn new(initial: Duration, max: Duration) -> Self {
        Self {
            initial,
            max,
            next: initial,
        }
    }

    /// The BLE bridge ladder: 3s doubling up to 15s (Python parity).
    pub const fn ble() -> Self {
        Self::new(Duration::from_secs(3), Duration::from_secs(15))
    }

    /// The delay to wait before the next attempt; doubles the pending delay
    /// (capped at `max`) for the attempt after that.
    pub fn next_delay(&mut self) -> Duration {
        let delay = self.next;
        self.next = self.next.saturating_mul(2).min(self.max);
        delay
    }

    /// The delay the next [`Self::next_delay`] call will return.
    pub fn current(&self) -> Duration {
        self.next
    }

    /// Restart the ladder (called after every successful connect, Python
    /// `self._reconnect_delay = _RECONNECT_S`).
    pub fn reset(&mut self) {
        self.next = self.initial;
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::ble()
    }
}
