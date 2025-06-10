use std::{cell::RefCell, rc::Rc, sync::Arc};

use futures::{lock::Mutex, AsyncRead, AsyncWrite, SinkExt, StreamExt};
use wasm_bindgen::{prelude::Closure, JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    RtcIceCandidate, RtcIceCandidateInit, RtcIceConnectionState, RtcIceGatheringState,
    RtcPeerConnectionIceEvent, RtcPeerConnectionState, RtcSdpType, RtcSessionDescriptionInit,
    RtcSignalingState,
};

use crate::{
    browser::{
        protocol::pb::{signaling_message, SignalingMessage},
        SignalingStream,
    },
    connection::RtcPeerConnection,
    error::Error,
    Connection,
};

// Implementation of WebRTC signaling protocol between two peers. This implementation follows
// the specification here: https://github.com/libp2p/specs/blob/master/webrtc/webrtc.md.

/// Connection states for ICE connection, ICE gathering, signaling
/// and the peer connection.
#[derive(Clone)]
struct ConnectionState {
    pub(crate) ice_connection: RtcIceConnectionState,
    pub(crate) ice_gathering: RtcIceGatheringState,
    pub(crate) signaling: RtcSignalingState,
    pub(crate) peer_connection: RtcPeerConnectionState,
}

/// Callbacks for ICE connection, ICE gathering, peer connection signaling
/// and ice candidate retrieval.
struct ConnectionCallbacks {
    _ice_connection_callback: Closure<dyn FnMut(web_sys::Event)>,
    _ice_gathering_callback: Closure<dyn FnMut(web_sys::Event)>,
    _peer_connection_callback: Closure<dyn FnMut(web_sys::Event)>,
    _signaling_callback: Closure<dyn FnMut(web_sys::Event)>,
    _ice_candidate_callback: Option<Closure<dyn FnMut(RtcPeerConnectionIceEvent)>>,
}

/// Helper function to safely convert ice connection state
fn safe_ice_connection_state_from_js(js_val: JsValue) -> RtcIceConnectionState {
    if let Some(state_str) = js_val.as_string() {
        match state_str.as_str() {
            "new" => RtcIceConnectionState::New,
            "checking" => RtcIceConnectionState::Checking,
            "connected" => RtcIceConnectionState::Connected,
            "completed" => RtcIceConnectionState::Completed,
            "failed" => RtcIceConnectionState::Failed,
            "disconnected" => RtcIceConnectionState::Disconnected,
            "closed" => RtcIceConnectionState::Closed,
            _ => {
                tracing::warn!("Unknown ICE connection state: '{}', defaulting to New", state_str);
                RtcIceConnectionState::New
            }
        }
    } else {
        tracing::warn!("ICE connection state is not a string: {:?}", js_val);
        RtcIceConnectionState::New
    }
}

/// Helper function to safely convert ice gathering state
fn safe_ice_gathering_state_from_js(js_val: JsValue) -> RtcIceGatheringState {
    if let Some(state_str) = js_val.as_string() {
        match state_str.as_str() {
            "new" => RtcIceGatheringState::New,
            "gathering" => RtcIceGatheringState::Gathering,
            "complete" => RtcIceGatheringState::Complete,
            _ => {
                tracing::warn!("Unknown ICE gathering state: '{}', defaulting to New", state_str);
                RtcIceGatheringState::New
            }
        }
    } else {
        tracing::warn!("ICE gathering state is not a string: {:?}", js_val);
        RtcIceGatheringState::New
    }
}

/// Helper function to safely convert peer connection state
fn safe_peer_connection_state_from_js(js_val: JsValue) -> RtcPeerConnectionState {
    if let Some(state_str) = js_val.as_string() {
        match state_str.as_str() {
            "new" => RtcPeerConnectionState::New,
            "connecting" => RtcPeerConnectionState::Connecting,
            "connected" => RtcPeerConnectionState::Connected,
            "disconnected" => RtcPeerConnectionState::Disconnected,
            "failed" => RtcPeerConnectionState::Failed,
            "closed" => RtcPeerConnectionState::Closed,
            _ => {
                tracing::warn!("Unknown peer connection state: '{}', defaulting to New", state_str);
                RtcPeerConnectionState::New
            }
        }
    } else {
        tracing::warn!("Peer connection state is not a string: {:?}", js_val);
        RtcPeerConnectionState::New
    }
}

/// Helper function to safely convert signaling state
fn safe_signaling_state_from_js(js_val: JsValue) -> RtcSignalingState {
    if let Some(state_str) = js_val.as_string() {
        match state_str.as_str() {
            "stable" => RtcSignalingState::Stable,
            "have-local-offer" => RtcSignalingState::HaveLocalOffer,
            "have-remote-offer" => RtcSignalingState::HaveRemoteOffer,
            "have-local-pranswer" => RtcSignalingState::HaveLocalPranswer,
            "have-remote-pranswer" => RtcSignalingState::HaveRemotePranswer,
            "closed" => RtcSignalingState::Closed,
            _ => {
                tracing::warn!("Unknown signaling state: '{}', defaulting to Closed", state_str);
                RtcSignalingState::Closed
            }
        }
    } else {
        tracing::warn!("Signaling state is not a string: {:?}", js_val);
        RtcSignalingState::Closed
    }
}

pub trait Signaling {
    /// Performs WebRTC signaling based on if the caller is the initiator.
    async fn perform_signaling(
        &self,
        stream: SignalingStream<impl AsyncRead + AsyncWrite + Unpin + 'static>,
        is_initiator: bool,
    ) -> Result<Connection, Error>;
}

/// Implementation of WebRTC signaling protocol.
pub struct SignalingProtocol {
    states: send_wrapper::SendWrapper<Rc<RefCell<ConnectionState>>>,
}

impl SignalingProtocol {
    pub fn new() -> Self {
        Self {
            states: send_wrapper::SendWrapper::new(Rc::new(RefCell::new(ConnectionState {
                ice_connection: RtcIceConnectionState::New,
                ice_gathering: RtcIceGatheringState::New,
                signaling: RtcSignalingState::Closed,
                peer_connection: RtcPeerConnectionState::Closed,
            }))),
        }
    }
}

impl Signaling for SignalingProtocol {
    async fn perform_signaling(
        &self,
        stream: SignalingStream<impl AsyncRead + AsyncWrite + Unpin + 'static>,
        is_initiator: bool,
    ) -> Result<Connection, Error> {
        tracing::info!("Starting WebRTC signaling. initiator={}", is_initiator);
        let rtc_conn = RtcPeerConnection::new("sha-256".to_string()).await?;
        let connection = rtc_conn.inner();

        let pb_stream = Arc::new(Mutex::new(stream));
        let (ice_candidate_sender, mut ice_candidate_receiver) =
            futures::channel::mpsc::channel::<RtcIceCandidate>(100);

        // Setup callbacks for state management
        // ICE connection state callback
        let states = self.states.clone();
        let ice_connection_callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
            let target = event.target().unwrap();
            let pc = target.dyn_ref::<web_sys::RtcPeerConnection>().unwrap();
            let state_js = pc.ice_connection_state();
            let state = safe_ice_connection_state_from_js(state_js.into());
            
            tracing::debug!("ICE connection state changed to: {:?}", state);
            states.borrow_mut().ice_connection = state;
        }) as Box<dyn FnMut(web_sys::Event)>);

        connection.set_oniceconnectionstatechange(Some(&ice_connection_callback.as_ref().unchecked_ref()));

        // ICE gathering state callback
        let states = self.states.clone();
        let ice_gathering_callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
            let target = event.target().unwrap();
            let pc = target.dyn_ref::<web_sys::RtcPeerConnection>().unwrap();
            let state_js = pc.ice_gathering_state();
            let state = safe_ice_gathering_state_from_js(state_js.into());
            
            tracing::debug!("ICE gathering state changed to: {:?}", state);
            states.borrow_mut().ice_gathering = state;
        }) as Box<dyn FnMut(web_sys::Event)>);

        connection.set_onicegatheringstatechange(Some(&ice_gathering_callback.as_ref().unchecked_ref()));

        // Peer connection state callback
        let states = self.states.clone();
        let peer_connection_callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
            let target = event.target().unwrap();
            let pc = target.dyn_ref::<web_sys::RtcPeerConnection>().unwrap();
            let state_js = pc.connection_state();
            let state = safe_peer_connection_state_from_js(state_js.into());
            
            tracing::debug!("Peer connection state changed to: {:?}", state);
            states.borrow_mut().peer_connection = state;
        }) as Box<dyn FnMut(web_sys::Event)>);

        connection.set_onconnectionstatechange(Some(&peer_connection_callback.as_ref().unchecked_ref()));

        // Signaling state callback
        let states = self.states.clone();
        let signaling_callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
            let target = event.target().unwrap();
            let pc = target.dyn_ref::<web_sys::RtcPeerConnection>().unwrap();
            let state_js = pc.signaling_state();
            let state = safe_signaling_state_from_js(state_js.into());
            
            tracing::debug!("Signaling state changed to: {:?}", state);
            states.borrow_mut().signaling = state;
        }) as Box<dyn FnMut(web_sys::Event)>);

        connection.set_onsignalingstatechange(Some(&signaling_callback.as_ref().unchecked_ref()));

        // Create the callbacks struct to keep closures alive
        let mut callbacks = ConnectionCallbacks {
            _ice_connection_callback: ice_connection_callback,
            _ice_gathering_callback: ice_gathering_callback,
            _peer_connection_callback: peer_connection_callback,
            _signaling_callback: signaling_callback,
            _ice_candidate_callback: None,
        };

        if is_initiator {
            // Create a data channel to ensure ICE information is shared in the SDP
            let data_channel = connection.create_data_channel("init");

            // Set a callback to handle ice candidates
            let sender_for_closure = ice_candidate_sender.clone();
            let ice_candidate_callback =
                Closure::wrap(Box::new(move |event: RtcPeerConnectionIceEvent| {
                    if let Some(candidate) = event.candidate() {
                      
                        let candidate_for_task = candidate.clone();
                        let mut sender_for_task = sender_for_closure.clone();

                        spawn_local(async move {
                            if let Err(e) = sender_for_task.send(candidate_for_task).await {
                                tracing::error!("Failed to send ICE candidate: {:?}", e);
                            }
                        });
                    } else {
                        tracing::info!("Initiator: End of ICE candidates (null candidate)");
                    }
                })
                as Box<dyn FnMut(RtcPeerConnectionIceEvent)>);

            connection.set_onicecandidate(Some(ice_candidate_callback.as_ref().unchecked_ref()));
            callbacks._ice_candidate_callback = Some(ice_candidate_callback);

            let pb_stream_clone = Arc::clone(&pb_stream);
                
            // Send ICE candidates to remote peer through the signaling stream
            spawn_local(async move {
                while let Some(candidate) = ice_candidate_receiver.next().await {
                    let candidate_json = candidate.to_json();
                    let candidate_as_str = js_sys::JSON::stringify(&candidate_json).unwrap().as_string().unwrap_or_default();

                    let message = SignalingMessage {
                        r#type: signaling_message::Type::IceCandidate as i32,
                        data: candidate_as_str
                    };
    
                    let mut guard = pb_stream_clone.lock().await;
                    if let Err(e) = guard.write(message).await {
                        tracing::error!("Failure to write ICE candidate to signaling stream: {:?}", e);
                        break;
                    }
                }
            });

            // Create offer
            let offer = JsFuture::from(connection.create_offer()).await?;
            let offer_sdp = js_sys::Reflect::get(&offer, &JsValue::from_str("sdp"))?
                .as_string()
                .ok_or_else(|| Error::Js("Could not extract SDP from offer".to_string()))?;

            let offer_init = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
            offer_init.set_sdp(&offer_sdp);

            JsFuture::from(connection.set_local_description(&offer_init))
                .await
                .map_err(|_| Error::Js("Could not set local description".to_string()))?;

            // Write SDP offer to the signaling stream
            let message = SignalingMessage {
                r#type: signaling_message::Type::SdpOffer as i32,
                data: offer_sdp,
            };
            pb_stream.lock().await.write(message).await.map_err(|_| {
                Error::ProtoSerialization(
                    "Failure to write SDP offer to signaling stream".to_string(),
                )
            })?;

            // Read SDP answer
            let answer_message = pb_stream.lock().await.read().await.map_err(|_| {
                Error::ProtoSerialization(
                    "Failure to read SDP answer from signaling stream".to_string(),
                )
            })?;
            let answer_init = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
            answer_init.set_sdp(&answer_message.data);

            // Set answer as remote description
            JsFuture::from(connection.set_remote_description(&answer_init))
                .await
                .map_err(|_| Error::Js("Could not set remote description".to_string()))?;

            // Receive and add ice candidates from remote peer until connected
            let connection_clone = connection.clone();
            spawn_local(async move {
                while let Ok(message) = pb_stream.lock().await.read().await {
                    if message.r#type == signaling_message::Type::IceCandidate as i32 {
                        if let Ok(candidate_json) = serde_json::from_str::<serde_json::Value>(&message.data) {
                            if let Some(candidate_str) = candidate_json.get("candidate").and_then(|v| v.as_str()) {
                                let candidate_init = RtcIceCandidateInit::new(&candidate_str);

                                if let Some(sdp_mid) = candidate_json.get("sdpMid").and_then(|v| v.as_str()) {
                                    candidate_init.set_sdp_mid(Some(sdp_mid));
                                }

                                if let Some(sdp_m_line_index) = candidate_json.get("sdpMLineIndex").and_then(|v| v.as_u64()) {
                                    candidate_init.set_sdp_m_line_index(Some(sdp_m_line_index as u16));
                                }

                                match JsFuture::from(
                                    connection_clone.add_ice_candidate_with_opt_rtc_ice_candidate_init(Some(&candidate_init))
                                ).await {
                                    Ok(_) => {}
                                    Err(e) => tracing::error!("Initiator: Failed to add remote ICE candidate: {:?}", e),
                                }
                            }
                        }

                    }
                }
            });

            data_channel.close();
        } else {
            // Read SDP offer
            let offer_message = pb_stream.lock().await.read().await.map_err(|_| {
                Error::ProtoSerialization(
                    "Failure to read SDP offer from signaling stream".to_string(),
                )
            })?;

            // Set remote description with remote offer
            let offer_init = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
            offer_init.set_sdp(&offer_message.data);
            
            JsFuture::from(connection.set_remote_description(&offer_init))
                .await
                .map_err(|_| Error::Js("Could not set remote description".to_string()))?;

            // Create answer and set local description
            let answer = JsFuture::from(connection.create_answer()).await?;
            let answer_sdp = js_sys::Reflect::get(&answer, &JsValue::from_str("sdp"))?
                .as_string()
                .ok_or_else(|| Error::Js("Could not extract SDP from answer".to_string()))?;

            let answer_init = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
            answer_init.set_sdp(&answer_sdp);
            
            JsFuture::from(connection.set_local_description(&answer_init))
                .await
                .map_err(|_| Error::Js("Could not set local description".to_string()))?;

            // Send SDP answer
            let answer_message = SignalingMessage {
                r#type: signaling_message::Type::SdpAnswer as i32,
                data: answer_sdp,
            };

            pb_stream.lock().await.write(answer_message).await.map_err(|_| {
                Error::ProtoSerialization(
                    "Failure to write SDP answer to signaling stream".to_string(),
                )
            })?;

            // Set up ICE candidate callback for non-initiator
            let sender_for_closure = ice_candidate_sender.clone();
            let ice_candidate_callback =
                Closure::wrap(Box::new(move |event: RtcPeerConnectionIceEvent| {
                    if let Some(candidate) = event.candidate() {
                        tracing::trace!("Non-initiator: Generated ICE candidate: {}", 
                            candidate.candidate());
                            let candidate_for_task = candidate.clone();
                        let mut sender_for_task = sender_for_closure.clone();
                        spawn_local(async move {
                            tracing::trace!("Non-initiator sending ICE candidate");
                            if let Err(e) = sender_for_task.send(candidate_for_task).await {
                                tracing::error!("Failed to send ICE candidate: {:?}", e);
                            }
                        });
                    } else {
                        tracing::info!("Non-initiator: End of ICE candidates (null candidate)");
                    }
                })
                as Box<dyn FnMut(RtcPeerConnectionIceEvent)>);

            connection.set_onicecandidate(Some(ice_candidate_callback.as_ref().unchecked_ref()));
            callbacks._ice_candidate_callback = Some(ice_candidate_callback);

            // Send ICE candidates to remote peer
            let ice_sender_stream = pb_stream.clone();
            spawn_local(async move {
                while let Some(candidate) = ice_candidate_receiver.next().await {
                    let candidate_json = candidate.to_json();
                    let candidate_as_str = js_sys::JSON::stringify(&candidate_json).unwrap().as_string().unwrap_or_default();

                    let message = SignalingMessage {
                        r#type: signaling_message::Type::IceCandidate as i32,
                        data: candidate_as_str
                    };
         
                    if let Err(e) = ice_sender_stream.lock().await.write(message).await {
                        tracing::error!("Failed to send ICE candidate: {:?}", e);
                        break;
                    }
                }
            });

            // Receive and add ice candidates from remote peer
            let ice_reader_stream = pb_stream.clone();
            let connection_clone = connection.clone();
            spawn_local(async move {
                while let Ok(message) = ice_reader_stream.lock().await.read().await {
                    if message.r#type == signaling_message::Type::IceCandidate as i32 {
                        tracing::trace!("Non-initiator: Received remote ICE candidate: {}", message.data);
                        if let Ok(candidate_json) = serde_json::from_str::<serde_json::Value>(&message.data) {
                            if let Some(candidate_str) = candidate_json.get("candidate").and_then(|v| v.as_str()) {
                                let candidate_init = RtcIceCandidateInit::new(&candidate_str);

                                if let Some(sdp_mid) = candidate_json.get("sdpMid").and_then(|v| v.as_str()) {
                                    candidate_init.set_sdp_mid(Some(sdp_mid));
                                }

                                if let Some(sdp_m_line_index) = candidate_json.get("sdpMLineIndex").and_then(|v| v.as_u64()) {
                                    candidate_init.set_sdp_m_line_index(Some(sdp_m_line_index as u16));
                                }

                                match JsFuture::from(
                                    connection_clone.add_ice_candidate_with_opt_rtc_ice_candidate_init(Some(&candidate_init))
                                ).await {
                                    Ok(_) => {},
                                    Err(e) => tracing::error!("Non Initiator: Failed to add remote ICE candidate: {:?}", e),
                                }
                            }
                        }
                    }
                }
            });
        }

        // Wait for the connection to be established. (30 seconds with 100ms intervals)
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 300; 
        
        loop {
            let current_states = self.states.borrow().clone();
            
            tracing::debug!("Connection status check #{}: ICE={:?}, Peer={:?}, Signaling={:?}, Gathering={:?}", 
                attempts + 1, 
                current_states.ice_connection,
                current_states.peer_connection, 
                current_states.signaling,
                current_states.ice_gathering
            );
        
            match current_states.peer_connection {
                RtcPeerConnectionState::Connected => {
                    tracing::info!("Peer connection is connected");
                    break;
                },
                RtcPeerConnectionState::Failed => {
                    tracing::error!("Peer connection failed");
                    return Err(Error::Signaling("Peer connection failed".to_string()));
                }
                _ => {
                    attempts += 1;
                    if attempts >= MAX_ATTEMPTS {
                        tracing::error!("Final states: ICE={:?}, Peer={:?}, Signaling={:?}, Gathering={:?}", 
                            current_states.ice_connection,
                            current_states.peer_connection, 
                            current_states.signaling,
                            current_states.ice_gathering
                        );
                        return Err(Error::Signaling("Connection timeout".to_string()));
                    }

                    gloo_timers::future::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }

        // Keep callbacks alive until connection is established
        drop(callbacks);

        tracing::info!("Successfully created WebRTC connection");
        Ok(Connection::new(rtc_conn))
    }
}