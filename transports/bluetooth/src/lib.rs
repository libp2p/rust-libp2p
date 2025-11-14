//! Bluetooth transport implementation for libp2p.
//!
//! This transport uses btleplug for cross-platform BLE support, providing
//! Central (dialing) and Peripheral (listening) roles.

mod common;
mod framing;

// Platform-specific peripheral implementations
#[cfg(target_os = "macos")]
mod peripheral_macos;

#[cfg(not(test))]
mod platform;

// Use mock for tests
#[cfg(test)]
mod mock;

pub use common::{BluetoothAddr, BluetoothAddrParseError, BluetoothTransportError};

#[cfg(not(test))]
pub use platform::*;

#[cfg(test)]
pub use mock::*;
