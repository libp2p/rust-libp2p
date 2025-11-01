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
    subscribed_centrals: Vec<Retained<CBCentral>>,
    tx_characteristic: Option<Retained<CBMutableCharacteristic>>,
    rx_characteristic: Option<Retained<CBMutableCharacteristic>>,
    ready_to_send: bool,
    frame_codec: FrameCodec,
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

                // Try to send any queued data
                self.send_queued_data(peripheral);
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
                log::debug!("Received {} write request(s)", requests.count());

                for i in 0..requests.count() {
                    let request = requests.objectAtIndex(i);
                    if let Some(value) = request.value() {
                        let bytes: &[u8] = value.bytes();

                        log::debug!("Received write: {} bytes", bytes.len());

                        // Process the data through frame codec
                        let mut state = self.ivars().state.lock();
                        state.frame_codec.push_data(bytes);

                        while let Ok(Some(frame)) = state.frame_codec.decode_next() {
                            log::debug!("Decoded frame: {} bytes", frame.len());
                            let _ = state.incoming_tx.try_send(frame);
                        }
                    }
                }

                // Respond success to all requests
                if requests.count() > 0 {
                    if let Some(first_request) = requests.firstObject() {
                        peripheral.respondToRequest_withResult(first_request.as_ref(), CBATTError::Success);
                    }
                }
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

            self.send_queued_data(peripheral);
        }
    }
);

impl PeripheralManagerDelegate {
    pub(crate) fn new(incoming_tx: mpsc::Sender<Vec<u8>>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PeripheralManagerDelegateIvars {
            state: Mutex::new(PeripheralState {
                incoming_tx,
                outgoing_queue: Vec::new(),
                subscribed_centrals: Vec::new(),
                tx_characteristic: None,
                rx_characteristic: None,
                ready_to_send: false,
                frame_codec: FrameCodec::new(),
            }),
        });
        unsafe { msg_send_id![super(this), init] }
    }

    fn setup_service(&self, peripheral: &CBPeripheralManager) {
        log::info!("Setting up BLE service and characteristics");

        // Create service UUID
        let service_uuid = uuid_to_cbuuid(&LIBP2P_SERVICE_UUID);
        let service = unsafe {
            CBMutableService::initWithType_primary(CBMutableService::alloc(), &service_uuid, true)
        };

        // Create RX characteristic (central writes to this)
        let rx_uuid = uuid_to_cbuuid(&RX_CHARACTERISTIC_UUID);
        let rx_properties = CBCharacteristicProperties::CBCharacteristicPropertyWrite
            | CBCharacteristicProperties::CBCharacteristicPropertyWriteWithoutResponse;
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
        log::info!("Starting BLE advertising");

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

    fn send_queued_data(&self, peripheral: &CBPeripheralManager) {
        let mut state = self.ivars().state.lock();

        if !state.ready_to_send || state.subscribed_centrals.is_empty() {
            return;
        }

        let Some(tx_char) = state.tx_characteristic.clone() else {
            return;
        };

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
    _peripheral: Retained<CBPeripheralManager>,
    _delegate: Retained<PeripheralManagerDelegate>,
    _outgoing_rx: Mutex<mpsc::Receiver<Vec<u8>>>,
}

impl BlePeripheralManager {
    /// Create a new BLE peripheral manager
    pub(crate) async fn new(
        incoming_tx: mpsc::Sender<Vec<u8>>,
        outgoing_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Result<Arc<Self>, Box<dyn std::error::Error>> {
        log::info!("Creating BLE peripheral manager");

        let delegate = PeripheralManagerDelegate::new(incoming_tx);

        let peripheral: Retained<CBPeripheralManager> = unsafe {
            msg_send_id![
                CBPeripheralManager::alloc(),
                initWithDelegate: Some(ProtocolObject::<dyn CBPeripheralManagerDelegate>::from_ref(&*delegate)),
                queue: std::ptr::null::<objc2::runtime::AnyObject>()
            ]
        };

        let manager = Arc::new(Self {
            _peripheral: peripheral,
            _delegate: delegate,
            _outgoing_rx: Mutex::new(outgoing_rx),
        });

        // Note: We cannot spawn a task for outgoing data because CBPeripheralManager
        // is not Send. The outgoing data handling happens in the delegate callbacks.
        // For a production implementation, we would need to use dispatch_queue_t
        // or ensure all CoreBluetooth operations happen on the main thread.

        Ok(manager)
    }
}

unsafe impl Send for BlePeripheralManager {}
unsafe impl Sync for BlePeripheralManager {}

/// Convert a UUID to CBUUID
fn uuid_to_cbuuid(uuid: &Uuid) -> Retained<CBUUID> {
    let uuid_str = NSString::from_str(&uuid.to_string());
    unsafe { CBUUID::UUIDWithString(&uuid_str) }
}
