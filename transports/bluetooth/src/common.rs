use std::{error, fmt, str::FromStr};

use libp2p_core::multiaddr::{Multiaddr, Protocol};

/// Bluetooth MAC address wrapper used by the in-memory mock transport as well as
/// platform specific implementations.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BluetoothAddr(pub(crate) [u8; 6]);

impl BluetoothAddr {
    pub fn new(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }

    pub fn is_unspecified(&self) -> bool {
        self.0.iter().all(|b| *b == 0)
    }

    pub(crate) fn into_u64(self) -> u64 {
        let mut bytes = [0u8; 8];
        bytes[2..].copy_from_slice(&self.0);
        u64::from_be_bytes(bytes)
    }

    pub fn to_multiaddr(self) -> Multiaddr {
        Protocol::Memory(self.into_u64()).into()
    }
}

impl fmt::Display for BluetoothAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl FromStr for BluetoothAddr {
    type Err = BluetoothAddrParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split(':');
        let mut bytes = [0u8; 6];
        for byte in bytes.iter_mut() {
            let part = parts.next().ok_or(BluetoothAddrParseError::InvalidFormat)?;
            if part.len() != 2 {
                return Err(BluetoothAddrParseError::InvalidFormat);
            }
            *byte =
                u8::from_str_radix(part, 16).map_err(|_| BluetoothAddrParseError::InvalidFormat)?;
        }
        if parts.next().is_some() {
            return Err(BluetoothAddrParseError::InvalidFormat);
        }
        Ok(Self(bytes))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BluetoothAddrParseError {
    InvalidFormat,
}

impl fmt::Display for BluetoothAddrParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid bluetooth address format")
    }
}

impl error::Error for BluetoothAddrParseError {}

/// Error that can be produced from the `BluetoothTransport`.
#[derive(Debug, Copy, Clone)]
pub enum BluetoothTransportError {
    /// There's no listener for the requested address.
    Unreachable,
    /// Tried to listen on an address that is already registered.
    AlreadyInUse,
    /// The current platform does not provide a Bluetooth transport implementation.
    Unsupported,
    /// Failed to establish a connection to a remote peer.
    ConnectionFailed,
}

impl fmt::Display for BluetoothTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            BluetoothTransportError::Unreachable => {
                write!(f, "No listener for the given bluetooth address.")
            }
            BluetoothTransportError::AlreadyInUse => {
                write!(f, "Bluetooth address already in use.")
            }
            BluetoothTransportError::Unsupported => {
                write!(f, "Bluetooth transport not supported on this platform.")
            }
            BluetoothTransportError::ConnectionFailed => {
                write!(f, "Failed to establish Bluetooth connection.")
            }
        }
    }
}

impl error::Error for BluetoothTransportError {}
