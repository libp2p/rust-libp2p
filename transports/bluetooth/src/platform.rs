//! Cross-platform Bluetooth transport implementation using SimpleBLE + CoreBluetooth.
//!
//! This module provides a dual-role BLE transport for libp2p:
//! - SimpleBLE for the central (dialing) role
//! - CoreBluetooth for the peripheral (listening) role on macOS

use std::{
    collections::{HashMap, VecDeque},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
    time::Duration,
};

use futures::{
    channel::mpsc,
    future::{self, Ready},
    prelude::*,
    StreamExt,
};
use libp2p_core::{
    multiaddr::Multiaddr,
    transport::{DialOpts, ListenerId, Transport, TransportError, TransportEvent},
};
use parking_lot::Mutex;
use rw_stream_sink::RwStreamSink;
use simplersble::{Adapter, Peripheral, ScanEvent};

use crate::common::BluetoothTransportError;
use crate::framing::FrameCodec;

#[cfg(target_os = "macos")]
use crate::peripheral_macos::BlePeripheralManager;

/// libp2p BLE service UUID - this identifies our service
const LIBP2P_SERVICE_UUID: &str = "00001234-0000-1000-8000-00805f9b34fb";

/// Characteristic UUID for RX (receiving data from peer - we read/subscribe)
const RX_CHARACTERISTIC_UUID: &str = "00001235-0000-1000-8000-00805f9b34fb";

/// Characteristic UUID for TX (transmitting data to peer - we write)
const TX_CHARACTERISTIC_UUID: &str = "00001236-0000-1000-8000-00805f9b34fb";

pub type Channel<T> = RwStreamSink<Chan<T>>;

fn scan_timeout_duration() -> Duration {
    const ENV_VAR: &str = "LIBP2P_BLUETOOTH_SCAN_TIMEOUT_SECS";
    match std::env::var(ENV_VAR) {
        Ok(value) => match value.parse::<u64>() {
            Ok(secs) => Duration::from_secs(secs),
            Err(err) => {
                log::warn!(
                    "Failed to parse {}='{}' as seconds: {}. Falling back to default.",
                    ENV_VAR,
                    value,
                    err
                );
                Duration::from_secs(20)
            }
        },
        Err(std::env::VarError::NotPresent) => Duration::from_secs(20),
        Err(err) => {
            log::warn!(
                "Could not read {}: {}. Falling back to default.",
                ENV_VAR,
                err
            );
            Duration::from_secs(20)
        }
    }
}

fn scan_collection_duration() -> Duration {
    const ENV_VAR: &str = "LIBP2P_BLUETOOTH_SCAN_COLLECTION_MS";
    match std::env::var(ENV_VAR) {
        Ok(value) => match value.parse::<u64>() {
            Ok(ms) => Duration::from_millis(ms),
            Err(err) => {
                log::warn!(
                    "Failed to parse {}='{}' as milliseconds: {}. Falling back to default.",
                    ENV_VAR,
                    value,
                    err
                );
                Duration::from_millis(5_000)
            }
        },
        Err(std::env::VarError::NotPresent) => Duration::from_millis(5_000),
        Err(err) => {
            log::warn!(
                "Could not read {}: {}. Falling back to default.",
                ENV_VAR,
                err
            );
            Duration::from_millis(5_000)
        }
    }
}

/// Channel implementation for BLE connections
pub struct Chan<T = Vec<u8>> {
    incoming: mpsc::Receiver<T>,
    outgoing: mpsc::Sender<T>,
}

impl<T> Unpin for Chan<T> {}

impl<T> Stream for Chan<T> {
    type Item = Result<T, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Stream::poll_next(Pin::new(&mut self.incoming), cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(item)) => Poll::Ready(Some(Ok(item))),
            Poll::Ready(None) => Poll::Ready(None),
        }
    }
}

impl<T> Sink<T> for Chan<T> {
    type Error = std::io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_ready(Pin::new(&mut self.outgoing), cx).map_err(map_channel_err)
    }

    fn start_send(mut self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        Sink::start_send(Pin::new(&mut self.outgoing), item).map_err(map_channel_err)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_flush(Pin::new(&mut self.outgoing), cx).map_err(map_channel_err)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Sink::poll_close(Pin::new(&mut self.outgoing), cx).map_err(map_channel_err)
    }
}

fn map_channel_err(error: mpsc::SendError) -> std::io::Error {
    if error.is_full() {
        std::io::Error::new(std::io::ErrorKind::WouldBlock, error)
    } else {
        std::io::Error::new(std::io::ErrorKind::BrokenPipe, "channel closed")
    }
}

/// Bluetooth transport using SimpleBLE
pub struct BluetoothTransport {
    inner: Arc<Mutex<TransportState>>,
    waker: Arc<Mutex<Option<Waker>>>,
}

struct TransportState {
    adapter: Option<Adapter>,
    #[cfg(target_os = "macos")]
    peripheral_manager: Option<Arc<BlePeripheralManager>>,
    listeners: HashMap<ListenerId, Listener>,
    connections: HashMap<String, Connection>,
}

struct Listener {
    id: ListenerId,
    addr: Multiaddr,
    incoming: VecDeque<(Channel<Vec<u8>>, Multiaddr)>,
    peripheral_incoming_rx: Option<mpsc::Receiver<Vec<u8>>>,
    peripheral_outgoing_tx: Option<mpsc::Sender<Vec<u8>>>,
    announced: bool,
    // Active connection's incoming channel for forwarding peripheral data
    active_connection_tx: Option<mpsc::Sender<Vec<u8>>>,
}

struct Connection {
    _peripheral: Peripheral,
    _service_uuid: String,
    _tx_char_uuid: String,
    _rx_char_uuid: String,
}

impl BluetoothTransport {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TransportState {
                adapter: None,
                #[cfg(target_os = "macos")]
                peripheral_manager: None,
                listeners: HashMap::new(),
                connections: HashMap::new(),
            })),
            waker: Arc::new(Mutex::new(None)),
        }
    }

    /// Initialize the BLE adapter (lazy initialization)
    async fn ensure_adapter(
        inner: &Arc<Mutex<TransportState>>,
    ) -> Result<Adapter, BluetoothTransportError> {
        // Check if we already have an adapter
        {
            let state = inner.lock();
            if let Some(adapter) = state.adapter.as_ref() {
                return Ok(adapter.clone());
            }
        }

        // Get available adapters
        let adapters = Adapter::get_adapters().map_err(|e| {
            log::error!("Failed to get BLE adapters: {:?}", e);
            BluetoothTransportError::Unsupported
        })?;

        if adapters.is_empty() {
            log::error!("No BLE adapters found");
            return Err(BluetoothTransportError::Unsupported);
        }

        let adapter = adapters.into_iter().next().unwrap();
        log::info!("Initialized BLE adapter");

        // Store the adapter
        {
            let mut state = inner.lock();
            state.adapter = Some(adapter.clone());
        }

        Ok(adapter)
    }
}

impl Clone for BluetoothTransport {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            waker: Arc::clone(&self.waker),
        }
    }
}

impl Transport for BluetoothTransport {
    type Output = Channel<Vec<u8>>;
    type Error = BluetoothTransportError;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        log::info!("Starting BLE peripheral (listening) on {}", addr);

        let inner = Arc::clone(&self.inner);
        let waker = Arc::clone(&self.waker);

        // Spawn task to start peripheral
        tokio::spawn(async move {
            // Create channels for incoming/outgoing data from peripheral
            let (peripheral_incoming_tx, peripheral_incoming_rx) = mpsc::channel(32);
            let (peripheral_outgoing_tx, peripheral_outgoing_rx) = mpsc::channel(32);

            // Start peripheral manager on macOS
            #[cfg(target_os = "macos")]
            {
                let should_start = {
                    let state = inner.lock();
                    state.peripheral_manager.is_none()
                };

                if should_start {
                    match BlePeripheralManager::new(
                        peripheral_incoming_tx.clone(),
                        peripheral_outgoing_rx,
                    )
                    .await
                    {
                        Ok(peripheral) => {
                            {
                                let mut state = inner.lock();
                                state.peripheral_manager = Some(peripheral);
                                log::info!("Started BLE peripheral manager");
                            }

                            // Give CoreBluetooth time to initialize and start advertising
                            // The peripheral manager needs its dispatch queue to process callbacks
                            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                        }
                        Err(e) => {
                            log::error!("Failed to start peripheral: {:?}", e);
                            return;
                        }
                    }
                }
            }

            #[cfg(not(target_os = "macos"))]
            {
                log::warn!("BLE peripheral mode not supported on this platform");
            }

            // Update listener with peripheral channels
            let listener = Listener {
                id,
                addr: addr.clone(),
                incoming: VecDeque::new(),
                peripheral_incoming_rx: Some(peripheral_incoming_rx),
                peripheral_outgoing_tx: Some(peripheral_outgoing_tx),
                announced: false,
                active_connection_tx: None,
            };

            let mut state = inner.lock();
            state.listeners.insert(id, listener);
            drop(state);

            // Wake the transport to announce the new address
            if let Some(waker) = waker.lock().as_ref() {
                waker.wake_by_ref();
            }

            log::info!("Peripheral listening on {}", addr);
        });

        Ok(())
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        let mut state = self.inner.lock();
        state.listeners.remove(&id).is_some()
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        _opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        let inner = Arc::clone(&self.inner);

        log::info!("Dialing Bluetooth address: {}", addr);

        Ok(Box::pin(async move {
            // Ensure we have an adapter
            let adapter = Self::ensure_adapter(&inner).await?;

            // Start scanning
            log::info!("Starting BLE scan...");

            adapter.scan_start().map_err(|e| {
                log::error!("Failed to start scan: {:?}", e);
                BluetoothTransportError::ConnectionFailed
            })?;

            // Get scan events stream
            let mut scan_stream = adapter.on_scan_event();

            let scan_timeout = scan_timeout_duration();
            let collection_window = scan_collection_duration();

            // Wait for peripherals advertising our service
            log::info!(
                "Scanning for peripherals with service {} (timeout: {:?}, warmup: {:?})...",
                LIBP2P_SERVICE_UUID,
                scan_timeout,
                collection_window
            );

            let timeout_sleep = tokio::time::sleep(scan_timeout);
            tokio::pin!(timeout_sleep);

            let mut discovered_peripherals: Vec<Peripheral> = Vec::new();
            let mut found_peripheral: Option<Peripheral> = None;

            // Collect peripherals for a bit to avoid connecting to ourselves
            let collection_sleep = tokio::time::sleep(collection_window);
            tokio::pin!(collection_sleep);

            // First, collect all peripherals for the warmup duration
            loop {
                tokio::select! {
                    Some(event) = scan_stream.next() => {
                        match event {
                            Ok(ScanEvent::Found(peripheral)) => {
                                let id = peripheral.identifier().unwrap_or_else(|_| "unknown".to_string());
                                let address = peripheral.address().unwrap_or_else(|_| "unknown".to_string());

                                log::debug!("Found peripheral - ID: '{}', Address: '{}'", id, address);
                                discovered_peripherals.push(peripheral);
                            }
                            Ok(ScanEvent::Updated(_)) => {
                                // Ignore updates
                            }
                            Ok(ScanEvent::Start) => {
                                log::debug!("Scan started");
                            }
                            Ok(ScanEvent::Stop) => {
                                log::debug!("Scan stopped");
                            }
                            Err(e) => {
                                log::error!("Scan error: {:?}", e);
                            }
                        }
                    }
                    _ = &mut collection_sleep => {
                        log::info!("Collected {} peripherals", discovered_peripherals.len());
                        break;
                    }
                    _ = &mut timeout_sleep => {
                        adapter.scan_stop().ok();
                        log::error!("Scan timeout - no peripherals found");
                        return Err(BluetoothTransportError::ConnectionFailed);
                    }
                }
            }

            // Try to connect to each peripheral until we find one with our service
            for peripheral in discovered_peripherals {
                let id = peripheral
                    .identifier()
                    .unwrap_or_else(|_| "unknown".to_string());
                let address = peripheral
                    .address()
                    .unwrap_or_else(|_| "unknown".to_string());

                log::info!("Trying peripheral - ID: '{}', Address: '{}'", id, address);

                // Check if peripheral is already connected
                if peripheral.is_connected().unwrap_or(false) {
                    log::warn!("Peripheral is already connected, skipping...");
                    continue;
                }

                // Try to connect
                match peripheral.connect() {
                    Ok(_) => {
                        log::info!("Successfully connected to peripheral! Checking services...");

                        // Get services to verify this peripheral has our service
                        match peripheral.services() {
                            Ok(services) => {
                                log::info!(
                                    "Found {} service(s) on peripheral {}",
                                    services.len(),
                                    address
                                );

                                // Log all services for debugging
                                for s in &services {
                                    log::info!("  Service on {}: {}", address, s.uuid());
                                }

                                // Check if this peripheral has our libp2p service
                                let has_libp2p_service = services.iter().any(|s| {
                                    let uuid = s.uuid().to_lowercase();
                                    uuid == LIBP2P_SERVICE_UUID.to_lowercase()
                                });

                                if has_libp2p_service {
                                    log::info!(
                                        "✓ Peripheral {} has the libp2p service {}!",
                                        address,
                                        LIBP2P_SERVICE_UUID
                                    );
                                    found_peripheral = Some(peripheral);
                                    break;
                                } else {
                                    log::info!("✗ Peripheral {} doesn't have libp2p service (expected {}), disconnecting...", address, LIBP2P_SERVICE_UUID);
                                    peripheral.disconnect().ok();
                                    continue;
                                }
                            }
                            Err(e) => {
                                log::warn!("Failed to get services from peripheral {}: {:?}, disconnecting...", address, e);
                                peripheral.disconnect().ok();
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to connect to peripheral {}: {:?}, trying next...",
                            address,
                            e
                        );
                        continue;
                    }
                }
            }

            let peripheral = found_peripheral.ok_or_else(|| {
                log::error!("Could not find any peripherals with the libp2p service");
                BluetoothTransportError::ConnectionFailed
            })?;

            // Stop scanning after successful connection
            adapter.scan_stop().ok();

            log::info!(
                "Connected to peripheral with libp2p service! Discovering characteristics..."
            );

            // Get services (we already checked it has our service above)
            let services = peripheral.services().map_err(|e| {
                log::error!("Failed to get services: {:?}", e);
                BluetoothTransportError::ConnectionFailed
            })?;

            // Find our service
            let service = services
                .iter()
                .find(|s| {
                    let uuid = s.uuid().to_lowercase();
                    uuid == LIBP2P_SERVICE_UUID.to_lowercase()
                })
                .ok_or_else(|| {
                    log::error!(
                        "Service {} not found (this shouldn't happen)",
                        LIBP2P_SERVICE_UUID
                    );
                    BluetoothTransportError::ConnectionFailed
                })?;

            log::info!("Found libp2p service");

            // Find our characteristics
            let characteristics = service.characteristics();
            log::info!("Found {} characteristics", characteristics.len());

            let rx_char = characteristics
                .iter()
                .find(|c| {
                    let uuid = c.uuid().to_lowercase();
                    uuid == RX_CHARACTERISTIC_UUID.to_lowercase()
                })
                .ok_or_else(|| {
                    log::error!("RX characteristic not found");
                    log::info!("Available characteristics:");
                    for c in &characteristics {
                        log::info!("  - {}", c.uuid());
                    }
                    BluetoothTransportError::ConnectionFailed
                })?;

            let tx_char = characteristics
                .iter()
                .find(|c| {
                    let uuid = c.uuid().to_lowercase();
                    uuid == TX_CHARACTERISTIC_UUID.to_lowercase()
                })
                .ok_or_else(|| {
                    log::error!("TX characteristic not found");
                    BluetoothTransportError::ConnectionFailed
                })?;

            log::info!("Found RX and TX characteristics");

            // Create channels for this connection
            let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(32);
            let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(32);

            // Subscribe to notifications on TX characteristic (peripheral transmits, we receive)
            let service_uuid = service.uuid();
            let rx_char_uuid = rx_char.uuid();
            let tx_char_uuid = tx_char.uuid();

            // Clone peripheral for the notification task
            let peripheral_for_notify = peripheral.clone();
            let in_tx_clone = in_tx.clone();
            let service_uuid_for_notify = service_uuid.clone();
            let tx_char_uuid_for_notify = tx_char_uuid.clone();

            tokio::spawn(async move {
                match peripheral_for_notify
                    .notify(&service_uuid_for_notify, &tx_char_uuid_for_notify)
                {
                    Ok(mut notification_stream) => {
                        let mut frame_codec = FrameCodec::new();

                        while let Some(event) = notification_stream.next().await {
                            match event {
                                Ok(simplersble::ValueChangedEvent::ValueUpdated(data)) => {
                                    log::debug!("Received notification: {} bytes", data.len());
                                    frame_codec.push_data(&data);

                                    while let Ok(Some(frame)) = frame_codec.decode_next() {
                                        log::debug!("Decoded frame: {} bytes", frame.len());
                                        let _ = in_tx_clone.clone().try_send(frame);
                                    }
                                }
                                Err(e) => {
                                    log::error!("Notification error: {:?}", e);
                                    break;
                                }
                            }
                        }

                        log::info!("Notification stream ended");
                    }
                    Err(e) => {
                        log::error!("Failed to subscribe to notifications: {:?}", e);
                    }
                }
            });

            // Spawn task to handle outgoing writes to RX characteristic (peripheral receives, we write)
            let peripheral_for_write = peripheral.clone();
            let service_uuid_for_write = service_uuid.clone();
            let rx_char_uuid_for_write = rx_char_uuid.clone();

            tokio::spawn(async move {
                // Longer delay to ensure connection and services are fully ready
                // SimpleBLE write_request needs everything to be stable
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

                while let Some(data) = out_rx.next().await {
                    log::debug!("Sending {} bytes", data.len());

                    // Encode the frame
                    let encoded = match FrameCodec::new().encode(&data) {
                        Ok(e) => e,
                        Err(e) => {
                            log::error!("Failed to encode frame: {:?}", e);
                            continue;
                        }
                    };

                    // Write the data to RX characteristic (peripheral receives)
                    // Use write_request (with response) to trigger didReceiveWriteRequests on peripheral
                    let mut retry_count = 0;
                    let max_retries = 3;

                    loop {
                        match peripheral_for_write.write_request(
                            &service_uuid_for_write,
                            &rx_char_uuid_for_write,
                            &encoded,
                        ) {
                            Ok(_) => {
                                log::debug!("Wrote {} bytes", encoded.len());
                                break;
                            }
                            Err(e) => {
                                retry_count += 1;
                                if retry_count >= max_retries {
                                    log::error!(
                                        "Failed to write after {} attempts: {:?}",
                                        max_retries,
                                        e
                                    );
                                    break;
                                }
                                log::warn!(
                                    "Write attempt {} failed, retrying: {:?}",
                                    retry_count,
                                    e
                                );
                                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                            }
                        }
                    }
                }

                log::info!("Outgoing write task ended");
            });

            // Store connection
            let peripheral_id = peripheral
                .identifier()
                .unwrap_or_else(|_| "unknown".to_string());
            {
                let mut state = inner.lock();
                state.connections.insert(
                    peripheral_id.clone(),
                    Connection {
                        _peripheral: peripheral.clone(),
                        _service_uuid: service_uuid,
                        _tx_char_uuid: tx_char_uuid,
                        _rx_char_uuid: rx_char_uuid,
                    },
                );
            }

            log::info!("Connection established to {}", peripheral_id);

            // Create the channel
            let channel = Channel::new(Chan {
                incoming: in_rx,
                outgoing: out_tx,
            });

            Ok(channel)
        }))
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        // Store the waker
        *self.waker.lock() = Some(cx.waker().clone());

        let mut state = self.inner.lock();

        // Check if any listeners need to announce their address
        for listener in state.listeners.values_mut() {
            if !listener.announced {
                listener.announced = true;
                let listen_addr = listener.addr.clone();
                let listener_id = listener.id;
                return Poll::Ready(TransportEvent::NewAddress {
                    listen_addr,
                    listener_id,
                });
            }

            // Check for incoming data from peripheral side
            // Process ALL available data, not just one packet
            if let Some(ref mut peripheral_rx) = listener.peripheral_incoming_rx {
                loop {
                    match peripheral_rx.poll_next_unpin(cx) {
                        Poll::Ready(Some(data)) => {
                            // Check if we have an active connection
                            if let Some(ref active_tx) = listener.active_connection_tx {
                                // Forward data to existing connection
                                log::debug!("Forwarding {} bytes to active connection", data.len());
                                if active_tx.clone().try_send(data).is_err() {
                                    log::warn!("Active connection closed, removing it");
                                    listener.active_connection_tx = None;
                                }
                            } else {
                                // No active connection, this is the first data - create a new connection
                                log::info!(
                                    "Received incoming peripheral connection with {} bytes",
                                    data.len()
                                );

                                // Get the outgoing sender for this listener
                                let outgoing_tx = listener.peripheral_outgoing_tx.clone().unwrap();

                                // Create new channels for libp2p
                                let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(32);
                                let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>(32);

                                // Forward the first data
                                let _ = in_tx.clone().try_send(data);

                                // Store the connection's incoming sender for future data forwarding
                                listener.active_connection_tx = Some(in_tx);

                                // Spawn task to forward outgoing data from connection to peripheral
                                let mut out_rx_stream = out_rx;
                                tokio::spawn(async move {
                                    while let Some(data) = out_rx_stream.next().await {
                                        let _ = outgoing_tx.clone().try_send(data);
                                    }
                                    log::info!("Peripheral outgoing data forwarding task ended");
                                });

                                // Create the channel
                                let channel = Channel::new(Chan {
                                    incoming: in_rx,
                                    outgoing: out_tx,
                                });

                                // Create a fake send_back_addr (peripheral connections don't have addresses)
                                let send_back_addr = listener.addr.clone();

                                listener.incoming.push_back((channel, send_back_addr));

                                // After creating connection, continue to process any buffered data
                                continue;
                            }
                        }
                        Poll::Ready(None) => {
                            // Peripheral channel closed
                            log::warn!("Peripheral incoming channel closed");
                            break;
                        }
                        Poll::Pending => {
                            // No more data available right now
                            break;
                        }
                    }
                }
            }

            // Check for queued incoming connections
            if let Some((channel, send_back_addr)) = listener.incoming.pop_front() {
                return Poll::Ready(TransportEvent::Incoming {
                    listener_id: listener.id,
                    upgrade: future::ready(Ok(channel)),
                    local_addr: listener.addr.clone(),
                    send_back_addr,
                });
            }
        }

        Poll::Pending
    }
}

impl Default for BluetoothTransport {
    fn default() -> Self {
        Self::new()
    }
}
