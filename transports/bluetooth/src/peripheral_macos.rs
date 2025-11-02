//! macOS BLE peripheral implementation using CoreBluetooth.
//!
//! This provides the peripheral (server) role for BLE, allowing the app to advertise
//! and accept incoming connections from centrals.

use std::sync::Arc;

use futures::channel::mpsc;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_core_bluetooth::{
    CBATTError, CBATTRequest, CBAdvertisementDataServiceUUIDsKey, CBCentral, CBCharacteristic,
    CBCharacteristicProperties, CBManagerState, CBMutableCharacteristic, CBMutableService,
    CBPeripheralManager, CBPeripheralManagerDelegate, CBUUID,
};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSError, NSObject, NSObjectProtocol, NSString,
};
use parking_lot::Mutex;
use uuid::Uuid;

use crate::framing::FrameCodec;

/// libp2p BLE service UUID
const LIBP2P_SERVICE_UUID: Uuid = Uuid::from_u128(0x00001234_0000_1000_8000_00805f9b34fb);

/// Characteristic UUID for RX (receiving data from central - they write)
const RX_CHARACTERISTIC_UUID: Uuid = Uuid::from_u128(0x00001235_0000_1000_8000_00805f9b34fb);

/// Characteristic UUID for TX (transmitting data to central - they read/subscribe)
const TX_CHARACTERISTIC_UUID: Uuid = Uuid::from_u128(0x00001236_0000_1000_8000_00805f9b34fb);

/// Shared state for the peripheral manager
struct PeripheralState {
    incoming_tx: mpsc::Sender<Vec<u8>>,
    outgoing_queue: Vec<Vec<u8>>,
    outgoing_data_rx: Option<mpsc::Receiver<Vec<u8>>>,
    subscribed_centrals: Vec<Retained<CBCentral>>,
    tx_characteristic: Option<Retained<CBMutableCharacteristic>>,
    rx_characteristic: Option<Retained<CBMutableCharacteristic>>,
    ready_to_send: bool,
    frame_codec: FrameCodec,
    peripheral_manager: Option<Retained<CBPeripheralManager>>,
}

pub(crate) struct PeripheralManagerDelegateIvars {
    state: Mutex<PeripheralState>,
}

declare_class!(
    pub(crate) struct PeripheralManagerDelegate;

    unsafe impl ClassType for PeripheralManagerDelegate {
        type Super = NSObject;
        type Mutability = mutability::InteriorMutable;
        const NAME: &'static str = "PeripheralManagerDelegate";
    }

    impl DeclaredClass for PeripheralManagerDelegate {
        type Ivars = PeripheralManagerDelegateIvars;
    }

    unsafe impl NSObjectProtocol for PeripheralManagerDelegate {}

    unsafe impl CBPeripheralManagerDelegate for PeripheralManagerDelegate {
        #[method(peripheralManagerDidUpdateState:)]
        fn peripheral_manager_did_update_state(&self, peripheral: &CBPeripheralManager) {
            unsafe {
                let state = peripheral.state();
                log::info!("Peripheral manager state changed: {:?}", state);

                if state == CBManagerState::PoweredOn {
                    log::info!("Peripheral manager powered on, starting setup");
                    self.setup_service(peripheral);
                }
            }
        }

        #[method(peripheralManager:didReceiveConnectionRequest:)]
        fn peripheral_manager_did_receive_connection_request(
            &self,
            _peripheral: &CBPeripheralManager,
            central: &CBCentral,
        ) {
            unsafe {
                log::info!("🟢 Received connection request from central: {}",
                    central.identifier().UUIDString());
            }
        }

        #[method(peripheralManager:willRestoreState:)]
        fn peripheral_manager_will_restore_state(
            &self,
            _peripheral: &CBPeripheralManager,
            _dict: &objc2_foundation::NSDictionary,
        ) {
            log::info!("Peripheral manager will restore state");
        }

        #[method(peripheralManager:didAddService:error:)]
        fn peripheral_manager_did_add_service(
            &self,
            _peripheral: &CBPeripheralManager,
            service: &objc2_core_bluetooth::CBService,
            error: Option<&NSError>,
        ) {
            unsafe {
                if let Some(error) = error {
                    log::error!("Failed to add service: {}", error.localizedDescription());
                } else {
                    log::info!("Service added successfully: {}", service.UUID().UUIDString());
                }
            }
        }

        #[method(peripheralManagerDidStartAdvertising:error:)]
        fn peripheral_manager_did_start_advertising(
            &self,
            _peripheral: &CBPeripheralManager,
            error: Option<&NSError>,
        ) {
            if let Some(error) = error {
                log::error!("Failed to start advertising: {}", error.localizedDescription());
            } else {
                log::info!("Started advertising successfully");
            }
        }

        #[method(peripheralManager:central:didSubscribeToCharacteristic:)]
        fn peripheral_manager_central_did_subscribe_to_characteristic(
            &self,
            peripheral: &CBPeripheralManager,
            central: &CBCentral,
            characteristic: &CBCharacteristic,
        ) {
            unsafe {
                log::info!(
                    "Central {} subscribed to characteristic {}",
                    central.identifier().UUIDString(),
                    characteristic.UUID().UUIDString()
                );

                let mut state = self.ivars().state.lock();
                if !state.subscribed_centrals.iter().any(|c| c.identifier() == central.identifier()) {
                    state.subscribed_centrals.push(central.retain());
                }
                state.ready_to_send = true;
                drop(state);

                // Check for new data and try to send any queued data
                self.check_and_send_data();
            }
        }

        #[method(peripheralManager:central:didUnsubscribeFromCharacteristic:)]
        fn peripheral_manager_central_did_unsubscribe_from_characteristic(
            &self,
            _peripheral: &CBPeripheralManager,
            central: &CBCentral,
            characteristic: &CBCharacteristic,
        ) {
            unsafe {
                log::info!(
                    "Central {} unsubscribed from characteristic {}",
                    central.identifier().UUIDString(),
                    characteristic.UUID().UUIDString()
                );

                let mut state = self.ivars().state.lock();
                state.subscribed_centrals.retain(|c| c.identifier() != central.identifier());
            }
        }

        #[method(peripheralManager:didReceiveReadRequest:)]
        fn peripheral_manager_did_receive_read_request(
            &self,
            peripheral: &CBPeripheralManager,
            request: &CBATTRequest,
        ) {
            unsafe {
                log::debug!("Received read request for characteristic {}",
                    request.characteristic().UUID().UUIDString());

                // For now, respond with empty data
                let data = NSData::new();
                request.setValue(Some(&data));
                peripheral.respondToRequest_withResult(request, CBATTError::Success);
            }
        }

        #[method(peripheralManager:didReceiveWriteRequests:)]
        fn peripheral_manager_did_receive_write_requests(
            &self,
            peripheral: &CBPeripheralManager,
            requests: &NSArray<CBATTRequest>,
        ) {
            unsafe {
                log::info!("🔵 Peripheral received {} write request(s)", requests.count());

                for i in 0..requests.count() {
                    let request = requests.objectAtIndex(i);
                    log::info!("  Request {} - characteristic: {}", i, request.characteristic().UUID().UUIDString());

                    if let Some(value) = request.value() {
                        let bytes: &[u8] = value.bytes();

                        log::info!("  Request {} - received {} bytes", i, bytes.len());

                        // Process the data through frame codec
                        let mut state = self.ivars().state.lock();
                        state.frame_codec.push_data(bytes);

                        while let Ok(Some(frame)) = state.frame_codec.decode_next() {
                            log::info!("  ✓ Decoded frame: {} bytes", frame.len());
                            let _ = state.incoming_tx.try_send(frame);
                        }
                    } else {
                        log::warn!("  Request {} has no value", i);
                    }
                }

                // Respond success to all requests
                if requests.count() > 0 {
                    if let Some(first_request) = requests.firstObject() {
                        log::info!("  Responding with Success to request");
                        peripheral.respondToRequest_withResult(first_request.as_ref(), CBATTError::Success);
                    }
                } else {
                    log::warn!("  No requests to respond to!");
                }

                // Check for and send any pending outgoing data
                // We do this after receiving data since it indicates an active connection
                self.check_and_send_data();
            }
        }

        #[method(peripheralManagerIsReadyToUpdateSubscribers:)]
        fn peripheral_manager_is_ready_to_update_subscribers(
            &self,
            peripheral: &CBPeripheralManager,
        ) {
            log::debug!("Peripheral manager ready to update subscribers");
            let mut state = self.ivars().state.lock();
            state.ready_to_send = true;
            drop(state);

            // Check for new data and send
            self.check_and_send_data();
        }
    }
);

impl PeripheralManagerDelegate {
    pub(crate) fn new(incoming_tx: mpsc::Sender<Vec<u8>>, outgoing_data_rx: mpsc::Receiver<Vec<u8>>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PeripheralManagerDelegateIvars {
            state: Mutex::new(PeripheralState {
                incoming_tx,
                outgoing_queue: Vec::new(),
                outgoing_data_rx: Some(outgoing_data_rx),
                subscribed_centrals: Vec::new(),
                tx_characteristic: None,
                rx_characteristic: None,
                ready_to_send: false,
                frame_codec: FrameCodec::new(),
                peripheral_manager: None,
            }),
        });
        unsafe { msg_send_id![super(this), init] }
    }

    pub(crate) fn set_peripheral_manager(&self, peripheral: Retained<CBPeripheralManager>) {
        let mut state = self.ivars().state.lock();
        state.peripheral_manager = Some(peripheral);
    }

    fn setup_service(&self, peripheral: &CBPeripheralManager) {
        log::info!("Setting up BLE service and characteristics");

        // Create service UUID
        let service_uuid = uuid_to_cbuuid(&LIBP2P_SERVICE_UUID);
        let service = unsafe {
            CBMutableService::initWithType_primary(CBMutableService::alloc(), &service_uuid, true)
        };

        // Create RX characteristic (central writes to this)
        // Use ONLY Write property (with response) to trigger didReceiveWriteRequests
        // WriteWithoutResponse does NOT trigger callbacks in peripheral mode
        let rx_uuid = uuid_to_cbuuid(&RX_CHARACTERISTIC_UUID);
        let rx_properties = CBCharacteristicProperties::CBCharacteristicPropertyWrite;
        let rx_char = unsafe {
            CBMutableCharacteristic::initWithType_properties_value_permissions(
                CBMutableCharacteristic::alloc(),
                &rx_uuid,
                rx_properties,
                None,
                objc2_core_bluetooth::CBAttributePermissions::Writeable,
            )
        };

        // Create TX characteristic (central subscribes to this)
        let tx_uuid = uuid_to_cbuuid(&TX_CHARACTERISTIC_UUID);
        let tx_properties = CBCharacteristicProperties::CBCharacteristicPropertyNotify
            | CBCharacteristicProperties::CBCharacteristicPropertyRead;
        let tx_char = unsafe {
            CBMutableCharacteristic::initWithType_properties_value_permissions(
                CBMutableCharacteristic::alloc(),
                &tx_uuid,
                tx_properties,
                None,
                objc2_core_bluetooth::CBAttributePermissions::Readable,
            )
        };

        // Store characteristics
        {
            let mut state = self.ivars().state.lock();
            state.tx_characteristic = Some(tx_char.clone());
            state.rx_characteristic = Some(rx_char.clone());
        }

        // Add characteristics to service
        unsafe {
            // Cast CBMutableCharacteristic to CBCharacteristic for the array
            let tx_char_base: Retained<CBCharacteristic> = std::mem::transmute(tx_char.clone());
            let rx_char_base: Retained<CBCharacteristic> = std::mem::transmute(rx_char.clone());
            let characteristics = NSArray::from_vec(vec![tx_char_base, rx_char_base]);
            service.setCharacteristics(Some(&*characteristics));

            // Add service to peripheral manager
            peripheral.addService(&service);
        }

        // Start advertising
        self.start_advertising(peripheral);
    }

    fn start_advertising(&self, peripheral: &CBPeripheralManager) {
        log::info!("Starting BLE advertising with service UUID: {}", LIBP2P_SERVICE_UUID);

        unsafe {
            let service_uuid = uuid_to_cbuuid(&LIBP2P_SERVICE_UUID);
            let service_uuids = NSArray::from_vec(vec![service_uuid]);

            // Create a simple advertisement with just the service UUID
            // We use msg_send to construct the dictionary manually
            let adv_data: Retained<NSDictionary<NSString, objc2::runtime::AnyObject>> = msg_send_id![
                NSDictionary::alloc(),
                initWithObjects: &[&*service_uuids as &objc2::runtime::AnyObject],
                forKeys: &[CBAdvertisementDataServiceUUIDsKey as &objc2::runtime::AnyObject],
                count: 1usize
            ];

            peripheral.startAdvertising(Some(&*adv_data));
        }
    }

    /// Process any pending outgoing data from the channel
    fn process_outgoing_channel(&self) -> bool {
        // Collect all pending data from the channel first
        let mut pending_data = Vec::new();
        {
            let mut state = self.ivars().state.lock();
            let Some(outgoing_rx) = state.outgoing_data_rx.as_mut() else {
                return false;
            };

            // Drain all available data from the channel without blocking
            while let Ok(Some(data)) = outgoing_rx.try_next() {
                pending_data.push(data);
            }
        }

        // Now process and encode the data
        if !pending_data.is_empty() {
            let mut state = self.ivars().state.lock();

            for data in pending_data {
                log::debug!("Processing outgoing data from channel: {} bytes", data.len());

                // Encode the data into a frame with length prefix
                match state.frame_codec.encode(&data) {
                    Ok(encoded_frame) => {
                        log::debug!("Queueing encoded frame: {} bytes", encoded_frame.len());
                        state.outgoing_queue.push(encoded_frame);
                    }
                    Err(e) => {
                        log::error!("Failed to encode outgoing data: {}", e);
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// Check for and send any pending outgoing data
    /// This should be called periodically to ensure data is sent
    pub(crate) fn check_and_send_data(&self) {
        // Process any pending data from the channel
        self.process_outgoing_channel();

        // Always try to send queued data, not just when there's new data
        // This ensures we retry sending when the peripheral manager becomes ready
        let peripheral = {
            let state = self.ivars().state.lock();
            state.peripheral_manager.clone()
        };

        if let Some(peripheral) = peripheral {
            self.send_queued_data(&peripheral);
        }
    }

    pub(crate) fn send_queued_data(&self, peripheral: &CBPeripheralManager) {
        // First, process any new data from the channel
        self.process_outgoing_channel();

        let mut state = self.ivars().state.lock();

        if !state.ready_to_send || state.subscribed_centrals.is_empty() {
            // Only log if there's actually data waiting to be sent
            if !state.outgoing_queue.is_empty() {
                log::debug!("Cannot send: ready={}, subscribers={}, queue={}",
                    state.ready_to_send, state.subscribed_centrals.len(), state.outgoing_queue.len());
            }
            return;
        }

        let Some(tx_char) = state.tx_characteristic.clone() else {
            if !state.outgoing_queue.is_empty() {
                log::debug!("No TX characteristic available, {} items queued", state.outgoing_queue.len());
            }
            return;
        };

        if !state.outgoing_queue.is_empty() {
            log::debug!("Attempting to send {} queued items", state.outgoing_queue.len());
        }

        while let Some(data) = state.outgoing_queue.first() {
            let ns_data = NSData::from_vec(data.clone());

            let success = unsafe {
                peripheral.updateValue_forCharacteristic_onSubscribedCentrals(
                    &ns_data, &tx_char, None, // Send to all subscribed centrals
                )
            };

            if success {
                log::debug!("Sent {} bytes via notification", data.len());
                state.outgoing_queue.remove(0);
            } else {
                log::debug!(
                    "Failed to send, queue has {} items",
                    state.outgoing_queue.len()
                );
                state.ready_to_send = false;
                break;
            }
        }
    }
}

/// BLE Peripheral manager for macOS
pub(crate) struct BlePeripheralManager {
    /// Keep peripheral alive - needed to maintain the BLE connection
    #[allow(dead_code)]
    peripheral: Retained<CBPeripheralManager>,
    /// Keep delegate alive - needed to receive callbacks
    #[allow(dead_code)]
    delegate: Retained<PeripheralManagerDelegate>,
}

impl BlePeripheralManager {
    /// Create a new BLE peripheral manager
    pub(crate) async fn new(
        incoming_tx: mpsc::Sender<Vec<u8>>,
        outgoing_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Result<Arc<Self>, String> {
        log::info!("Creating BLE peripheral manager");

        let delegate = PeripheralManagerDelegate::new(incoming_tx, outgoing_rx);

        // Use dispatch_get_global_queue to get a concurrent queue for CoreBluetooth
        // Using nil queue would use the main thread, which doesn't work well with Tokio
        let queue: *mut objc2::runtime::AnyObject = unsafe {
            use std::ffi::c_long;
            extern "C" {
                fn dispatch_get_global_queue(identifier: c_long, flags: usize) -> *mut objc2::runtime::AnyObject;
            }
            // QOS_CLASS_USER_INTERACTIVE = 0x21
            dispatch_get_global_queue(0x21, 0)
        };

        let peripheral: Retained<CBPeripheralManager> = unsafe {
            msg_send_id![
                CBPeripheralManager::alloc(),
                initWithDelegate: Some(ProtocolObject::<dyn CBPeripheralManagerDelegate>::from_ref(&*delegate)),
                queue: queue
            ]
        };

        // Set the peripheral manager reference in the delegate so it can trigger sends
        delegate.set_peripheral_manager(peripheral.clone());

        let manager = Arc::new(Self {
            peripheral,
            delegate: delegate.clone(),
        });

        // Set up a GCD timer to periodically check for outgoing data
        // This ensures we don't miss data due to timing issues
        unsafe {
            use std::ffi::c_void;
            extern "C" {
                fn dispatch_source_create(
                    type_: *const c_void,
                    handle: usize,
                    mask: usize,
                    queue: *mut objc2::runtime::AnyObject,
                ) -> *mut c_void;
                fn dispatch_source_set_timer(
                    source: *mut c_void,
                    start: u64,
                    interval: u64,
                    leeway: u64,
                );
                fn dispatch_source_set_event_handler_f(
                    source: *mut c_void,
                    handler: extern "C" fn(*mut c_void),
                );
                fn dispatch_set_context(object: *mut c_void, context: *mut c_void);
                fn dispatch_resume(object: *mut c_void);
                fn dispatch_get_global_queue(identifier: i64, flags: usize) -> *mut objc2::runtime::AnyObject;

                static _dispatch_source_type_timer: c_void;
            }

            // Create a timer on a global queue
            let timer_queue = dispatch_get_global_queue(0, 0);
            let timer = dispatch_source_create(
                &_dispatch_source_type_timer as *const _,
                0,
                0,
                timer_queue,
            );

            // Set timer to fire every 10ms
            let start = 0u64; // DISPATCH_TIME_NOW
            let interval = 10_000_000u64; // 10ms in nanoseconds
            let leeway = 1_000_000u64; // 1ms leeway
            dispatch_source_set_timer(timer, start, interval, leeway);

            // Store delegate as context
            let delegate_ptr = Box::into_raw(Box::new(delegate.clone())) as *mut c_void;
            dispatch_set_context(timer, delegate_ptr);

            // Set event handler
            extern "C" fn timer_handler(context: *mut c_void) {
                unsafe {
                    let delegate_ptr = context as *const Retained<PeripheralManagerDelegate>;
                    if !delegate_ptr.is_null() {
                        (*delegate_ptr).check_and_send_data();
                    }
                }
            }
            dispatch_source_set_event_handler_f(timer, timer_handler);

            // Start the timer
            dispatch_resume(timer);

            // Leak the timer - it will run for the lifetime of the program
            std::mem::forget(timer);
        }

        log::info!("Peripheral manager created with outgoing data handling");
        log::info!("Note: Outgoing data is sent reactively and polled every 10ms");

        Ok(manager)
    }

    /// Stop advertising and clean up the peripheral
    pub(crate) fn stop(&self) {
        log::info!("Stopping BLE peripheral manager");
        unsafe {
            self.peripheral.stopAdvertising();
            log::info!("Stopped BLE advertising");
        }
    }
}

impl Drop for BlePeripheralManager {
    fn drop(&mut self) {
        log::info!("Dropping BLE peripheral manager - stopping advertising");
        self.stop();
    }
}

unsafe impl Send for BlePeripheralManager {}
unsafe impl Sync for BlePeripheralManager {}

/// Convert a UUID to CBUUID
fn uuid_to_cbuuid(uuid: &Uuid) -> Retained<CBUUID> {
    let uuid_str = NSString::from_str(&uuid.to_string());
    unsafe { CBUUID::UUIDWithString(&uuid_str) }
}
