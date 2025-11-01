//! BLE Peripheral implementation using ble-peripheral-rust.
//!
//! This module provides the server/listening side of the BLE transport.
//!
//! Note: Due to platform limitations with ble-peripheral-rust (not Send/Sync),
//! this is currently a placeholder. A full implementation would need to either:
//! 1. Use a different peripheral library
//! 2. Run the peripheral in a dedicated local thread (not tokio)
//! 3. Use platform-specific implementations directly

use futures::channel::mpsc;
use uuid::Uuid;

use crate::framing::FrameCodec;

/// libp2p BLE service UUID
#[allow(dead_code)]
const LIBP2P_SERVICE_UUID: Uuid = Uuid::from_u128(0x00001234_0000_1000_8000_00805f9b34fb);

/// Characteristic UUID for RX (receiving data from central - they write)
#[allow(dead_code)]
const RX_CHARACTERISTIC_UUID: Uuid = Uuid::from_u128(0x00001235_0000_1000_8000_00805f9b34fb);

/// Characteristic UUID for TX (transmitting data to central - they read/notify)
#[allow(dead_code)]
const TX_CHARACTERISTIC_UUID: Uuid = Uuid::from_u128(0x00001236_0000_1000_8000_00805f9b34fb);

/// BLE Peripheral manager (placeholder)
pub struct BlePeripheralManager {
    _incoming_tx: mpsc::Sender<Vec<u8>>,
    _outgoing_rx: mpsc::Receiver<Vec<u8>>,
    _frame_codec: FrameCodec,
}

impl BlePeripheralManager {
    /// Create a new peripheral manager
    ///
    /// Note: This is currently a stub due to Send/Sync limitations with ble-peripheral-rust.
    /// A proper implementation would need platform-specific code or a different library.
    pub async fn new(
        incoming_tx: mpsc::Sender<Vec<u8>>,
        outgoing_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        log::warn!("BLE peripheral mode is not yet fully implemented");
        log::warn!("Central (dial) mode with btleplug is working, but peripheral (listen) needs more work");

        Ok(Self {
            _incoming_tx: incoming_tx,
            _outgoing_rx: outgoing_rx,
            _frame_codec: FrameCodec::new(),
        })
    }

    /// Send data to connected centrals
    #[allow(dead_code)]
    pub async fn send_data(&self, _data: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        log::warn!("BLE peripheral send_data not yet implemented");
        Ok(())
    }

    /// Stop advertising and cleanup
    #[allow(dead_code)]
    pub async fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        log::debug!("BLE peripheral stop (no-op)");
        Ok(())
    }
}
