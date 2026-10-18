//! M0 compatibility shim.
//!
//! The serial spike (port enumeration, HELLO probe, structured error
//! classification, HELLO parsing) moved into
//! [`crate::device::protocol`] when the M1 device layer landed. This module
//! re-exports those paths so the M0 command surface
//! (`src/commands/spikes.rs`) and the M0 tests keep working unchanged.

pub use crate::device::protocol::*;
