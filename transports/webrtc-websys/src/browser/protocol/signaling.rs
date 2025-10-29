use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

use futures::{lock::Mutex, AsyncRead, AsyncWrite};
use tracing::{info, instrument};
use wasm_bindgen::{prelude::Closure, JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    RtcIceCandidate, RtcIceCandidateInit, RtcIceConnectionState, RtcIceGatheringState,
    RtcPeerConnectionIceEvent, RtcPeerConnectionState, RtcSdpType, RtcSessionDescriptionInit,
    RtcSignalingState,
};

use crate::{
    browser::{
        protocol::pb::{signaling_message, SignalingMessage},
        SignalingConfig, SignalingStream,
    },
    connection::RtcPeerConnection,
    error::Error,
    Connection,
};

// Implementation of WebRTC signaling protocol. This implementation follows
// the specification here: https://github.com/libp2p/specs/blob/master/webrtc/webrtc.md.

/// Connection states for ICE connection, ICE gathering, signaling
/// and the peer connection.
#[derive(Clone, Debug)]
struct ConnectionState {
    pub(crate) ice_connection: RtcIceConnectionState,
    pub(crate) ice_gathering: RtcIceGatheringState,
    pub(crate) signaling: RtcSignalingState,
    pub(crate) peer_connection: RtcPeerConnectionState,
}

/// Callbacks for ICE connection, ICE gathering, peer connection signaling
/// and ice candidate retrieval.
#[derive(Debug)]
pub struct ConnectionCallbacks {
    _ice_connection_callback: Closure<dyn FnMut(web_sys::Event)>,
    _ice_gathering_callback: Closure<dyn FnMut(web_sys::Event)>,
    _peer_connection_callback: Closure<dyn FnMut(web_sys::Event)>,
    _signaling_callback: Closure<dyn FnMut(web_sys::Event)>,
}

/// Collected ICE candidates during the gathering phase.
#[derive(Debug, Clone)]
struct IceCandidateCollection {
    candidates: Vec<RtcIceCandidate>,
    gathering_complete: bool,
}

impl IceCandidateCollection {
    fn new() -> Self {
        Self {
            candidates: Vec::new(),
            gathering_complete: false,
        }
    }

    /// Adds an ice candidate to the candidates collection.
    fn add_candidate(&mut self, candidate: Option<RtcIceCandidate>) {
        match candidate {
            Some(candidate) => {
                tracing::trace!("Collected ICE candidate: {}", candidate.candidate());
                self.candidates.push(candidate);
            }
            None => {
                tracing::info!("ICE gathering complete");
                self.gathering_complete = true;
            }
        }
    }

    /// Returns whether the candidate gathering is complete.
    fn is_complete(&self) -> bool {
        self.gathering_complete
    }

    /// Removes and returns candidates from the candidates vector.
    fn drain_candidates(&mut self) -> Vec<RtcIceCandidate> {
        self.candidates.drain(..).collect()
    }
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
                tracing::warn!(
                    "Unknown ICE connection state: '{}', defaulting to New",
                    state_str
                );
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
                tracing::warn!(
                    "Unknown ICE gathering state: '{}', defaulting to New",
                    state_str
                );
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
                tracing::warn!(
                    "Unknown peer connection state: '{}', defaulting to New",
                    state_str
                );
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
                tracing::warn!(
                    "Unknown signaling state: '{}', defaulting to Closed",
                    state_str
                );
                RtcSignalingState::Closed
            }
        }
    } else {
        tracing::warn!("Signaling state is not a string: {:?}", js_val);
        RtcSignalingState::Closed
    }
}

pub trait Signaling {
    /// Performs WebRTC signaling as the initiator.
    async fn signaling_as_initiator(
        &self,
        stream: SignalingStream<impl AsyncRead + AsyncWrite + Unpin + 'static>,
    ) -> Result<Connection, Error>;

    /// Performs WebRTC signaling as the responder.
    async fn signaling_as_responder(
        &self,
        stream: SignalingStream<impl AsyncRead + AsyncWrite + Unpin + 'static>,
    ) -> Result<Connection, Error>;
}

#[derive(Debug)]
pub struct ProtocolHandler {
    states: send_wrapper::SendWrapper<Rc<RefCell<ConnectionState>>>,
    config: SignalingConfig,
}

impl ProtocolHandler {
    pub fn new(config: SignalingConfig) -> Self {
        Self {
            states: send_wrapper::SendWrapper::new(Rc::new(RefCell::new(ConnectionState {
                ice_connection: RtcIceConnectionState::New,
                ice_gathering: RtcIceGatheringState::New,
                signaling: RtcSignalingState::Closed,
                peer_connection: RtcPeerConnectionState::Closed,
            }))),
            config,
        }
    }
}

impl ProtocolHandler {
    /// Sets up the peer connection state callbacks including ICE connection, ICE gathering,
    /// peer connection and signaling.
    fn setup_peer_connection_state_callbacks(
        &self,
        connection: &web_sys::RtcPeerConnection,
    ) -> ConnectionCallbacks {
        tracing::trace!("Setting up peer connection state callbacks");

        // Setup callbacks for state management
        // ICE connection state callback
        let states = self.states.clone();
        let ice_connection_callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if let Some(target) = event.target() {
                if let Some(pc) = target.dyn_ref::<web_sys::RtcPeerConnection>() {
                    let state_js = pc.ice_connection_state();
                    let state = safe_ice_connection_state_from_js(state_js.into());

                    tracing::debug!("ICE connection state changed to: {:?}", state);
                    states.borrow_mut().ice_connection = state;
                }
            }
        }) as Box<dyn FnMut(web_sys::Event)>);

        connection.set_oniceconnectionstatechange(Some(
            &ice_connection_callback.as_ref().unchecked_ref(),
        ));

        // ICE gathering state callback
        let states = self.states.clone();
        let ice_gathering_callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if let Some(target) = event.target() {
                if let Some(pc) = target.dyn_ref::<web_sys::RtcPeerConnection>() {
                    let state_js = pc.ice_gathering_state();
                    let state = safe_ice_gathering_state_from_js(state_js.into());

                    tracing::debug!("ICE gathering state changed to: {:?}", state);
                    states.borrow_mut().ice_gathering = state;
                }
            }
        }) as Box<dyn FnMut(web_sys::Event)>);

        connection
            .set_onicegatheringstatechange(Some(&ice_gathering_callback.as_ref().unchecked_ref()));

        // Peer connection state callback
        let states = self.states.clone();
        let peer_connection_callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if let Some(target) = event.target() {
                if let Some(pc) = target.dyn_ref::<web_sys::RtcPeerConnection>() {
                    let state_js = pc.connection_state();
                    let state = safe_peer_connection_state_from_js(state_js.into());

                    tracing::debug!("Peer connection state changed to: {:?}", state);
                    states.borrow_mut().peer_connection = state;
                }
            }
        }) as Box<dyn FnMut(web_sys::Event)>);

        connection
            .set_onconnectionstatechange(Some(&peer_connection_callback.as_ref().unchecked_ref()));

        // Signaling state callback
        let states = self.states.clone();
        let signaling_callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if let Some(target) = event.target() {
                if let Some(pc) = target.dyn_ref::<web_sys::RtcPeerConnection>() {
                    let state_js = pc.signaling_state();
                    let state = safe_signaling_state_from_js(state_js.into());

                    tracing::debug!("Signaling state changed to: {:?}", state);
                    states.borrow_mut().signaling = state;
                }
            }
        }) as Box<dyn FnMut(web_sys::Event)>);

        connection.set_onsignalingstatechange(Some(&signaling_callback.as_ref().unchecked_ref()));

        // Create the callbacks struct to keep closures alive. Callbacks will be stored in the
        // Connection to be dropped when the Connection drops.
        ConnectionCallbacks {
            _ice_connection_callback: ice_connection_callback,
            _ice_gathering_callback: ice_gathering_callback,
            _peer_connection_callback: peer_connection_callback,
            _signaling_callback: signaling_callback,
        }
    }

    /// Sets up ICE candidate collection for finite signaling
    fn setup_ice_candidate_collection(
        &self,
        connection: &web_sys::RtcPeerConnection,
    ) -> (
        Rc<RefCell<IceCandidateCollection>>,
        Closure<dyn FnMut(RtcPeerConnectionIceEvent)>,
    ) {
        let ice_candidates = Rc::new(RefCell::new(IceCandidateCollection::new()));

        let ice_candidate_callback = {
            let ice_candidates = ice_candidates.clone();
            Closure::wrap(Box::new(move |event: RtcPeerConnectionIceEvent| {
                ice_candidates.borrow_mut().add_candidate(event.candidate());
            }) as Box<dyn FnMut(RtcPeerConnectionIceEvent)>)
        };

        connection.set_onicecandidate(Some(ice_candidate_callback.as_ref().unchecked_ref()));

        (ice_candidates, ice_candidate_callback)
    }

    /// Waits for ICE gathering to complete
    async fn wait_for_ice_gathering_complete(&self) -> Result<(), Error> {
        let mut attempts = 0;
        tracing::trace("Waiting for ice gathering to complete");

        loop {
            let current_state = self.states.borrow().ice_gathering.clone();

            match current_state {
                RtcIceGatheringState::Complete => {
                    tracing::info!("ICE gathering completed");
                    break;
                }
                RtcIceGatheringState::New | RtcIceGatheringState::Gathering => {
                    attempts += 1;
                    if attempts >= self.config.max_ice_gathering_attempts {
                        tracing::warn!("ICE gathering timeout, proceeding anyway");
                        break;
                    }
                    gloo_timers::future::sleep(Duration::from_millis(100)).await;
                }
                _ => {
                    tracing::warn!("Unexpected ICE gathering state: {:?}", current_state);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Waits for the RtcPeerConnectionState to change to a `Connected` state for a 
    /// configurable max number of attempts.
    async fn wait_for_established_conn(&self) -> Result<(), Error> {
        let mut attempts = 0;
        info!("Waiting for connection to establish");
        loop {
            let current_states = self.states.borrow().clone();

            tracing::debug!(
                "Connection status check #{}: ICE={:?}, Peer={:?}, Signaling={:?}, Gathering={:?}",
                attempts + 1,
                current_states.ice_connection,
                current_states.peer_connection,
                current_states.signaling,
                current_states.ice_gathering
            );

            match current_states.peer_connection {
                RtcPeerConnectionState::Connected => {
                    tracing::info!("Peer connection state transitioned to connected");
                    break;
                }
                RtcPeerConnectionState::Failed => {
                    tracing::error!("Peer connection state transitioned to failed");
                    return Err(Error::Signaling("Peer connection failed".to_string()));
                }
                _ => {
                    attempts += 1;
                    if attempts >= self.config.max_connection_establishment_checks {
                        tracing::error!(
                            "Final states: ICE={:?}, Peer={:?}, Signaling={:?}, Gathering={:?}",
                            current_states.ice_connection,
                            current_states.peer_connection,
                            current_states.signaling,
                            current_states.ice_gathering
                        );
                        return Err(Error::Signaling("Connection timeout".to_string()));
                    }

                    gloo_timers::future::sleep(
                        self.config.connection_establishment_delay_in_millis,
                    )
                    .await;
                }
            }
        }

        let current_states = self.states.borrow().clone();
        if current_states.peer_connection != web_sys::RtcPeerConnectionState::Connected {
            tracing::error!("Rtc peer connection failed. Connection not properly established.");
            return Err(Error::Signaling(
                "Connection not properly established".to_string(),
            ));
        }

        Ok(())
    }

    /// Parse ICE candidate from JSON message
    fn parse_ice_candidate(message: &SignalingMessage) -> Option<RtcIceCandidateInit> {
        if let Ok(candidate_json) = serde_json::from_str::<serde_json::Value>(&message.data) {
            if let Some(candidate_str) = candidate_json.get("candidate").and_then(|v| v.as_str()) {
                let candidate_init = RtcIceCandidateInit::new(candidate_str);

                if let Some(sdp_mid) = candidate_json.get("sdpMid").and_then(|v| v.as_str()) {
                    candidate_init.set_sdp_mid(Some(sdp_mid));
                }

                if let Some(sdp_m_line_index) =
                    candidate_json.get("sdpMLineIndex").and_then(|v| v.as_u64())
                {
                    candidate_init.set_sdp_m_line_index(Some(sdp_m_line_index as u16));
                }

                return Some(candidate_init);
            }
        }

        None
    }

    /// Sends collected ICE candidates through the signaling stream
    async fn send_ice_candidates(
        stream: &Arc<Mutex<SignalingStream<impl AsyncRead + AsyncWrite + Unpin + 'static>>>,
        candidates: Vec<RtcIceCandidate>,
    ) -> Result<(), Error> {
        tracing::info!("Sending {} ICE candidates", candidates.len());

        for candidate in candidates {
            let candidate_json = candidate.to_json();
            let candidate_as_str = js_sys::JSON::stringify(&candidate_json)
                .unwrap()
                .as_string()
                .unwrap_or_default();

            let message = SignalingMessage {
                r#type: signaling_message::Type::IceCandidate as i32,
                data: candidate_as_str,
            };

            stream.lock().await.write(message).await.map_err(|_| {
                Error::ProtoSerialization("Failed to send ICE candidate".to_string())
            })?;
        }

        // Send end-of-candidates marker as an empty JSON object
        let end_message = SignalingMessage {
            r#type: signaling_message::Type::IceCandidate as i32,
            data: "{}".to_string(),
        };

        stream.lock().await.write(end_message).await.map_err(|_| {
            Error::ProtoSerialization("Failed to send end-of-candidates marker".to_string())
        })?;

        tracing::info!("ICE candidate transmission complete");
        Ok(())
    }

    /// Receives ICE candidates from the signaling stream until end marker
    async fn receive_ice_candidates(
        stream: &Arc<Mutex<SignalingStream<impl AsyncRead + AsyncWrite + Unpin + 'static>>>,
        connection: &web_sys::RtcPeerConnection,
    ) -> Result<(), Error> {
        tracing::info!("Receiving ICE candidates from remote peer");

        loop {
            let message = stream.lock().await.read().await.map_err(|_| {
                Error::ProtoSerialization("Failed to read ICE candidate".to_string())
            })?;

            if message.r#type != signaling_message::Type::IceCandidate as i32 {
                return Err(Error::Signaling(
                    "Expected ICE candidate message".to_string(),
                ));
            }

            // Check for end-of-candidates marker
            if message.data == "{}" {
                tracing::info!("Received end-of-candidates marker");
                break;
            }

            tracing::trace!("Received remote ICE candidate: {}", message.data);

            if let Some(candidate_init) = Self::parse_ice_candidate(&message) {
                match JsFuture::from(
                    connection
                        .add_ice_candidate_with_opt_rtc_ice_candidate_init(Some(&candidate_init)),
                )
                .await
                {
                    Err(e) => {
                        tracing::error!("Failed to add remote ICE candidate: {:?}", e);
                    }
                    _ => {}
                }
            } else {
                tracing::warn!("Failed to parse ICE candidate: {}", message.data);
            }
        }

        tracing::info!("ICE candidate reception complete");
        Ok(())
    }
}

impl Signaling for ProtocolHandler {
    #[instrument(skip(stream), fields(initiator = false))]
    async fn signaling_as_responder(
        &self,
        stream: SignalingStream<impl AsyncRead + AsyncWrite + Unpin + 'static>,
    ) -> Result<Connection, Error> {
        tracing::info!("Starting WebRTC signaling as responder");
        
        let rtc_conn = RtcPeerConnection::new("sha-256".to_string(), self.config.stun_servers).await?;
        let connection = rtc_conn.inner();

        let pb_stream = Arc::new(Mutex::new(stream));
        let callbacks = self.setup_peer_connection_state_callbacks(connection);
        let (ice_candidates, _ice_callback) = self.setup_ice_candidate_collection(connection);

        // Read SDP offer
        let offer_message = pb_stream.lock().await.read().await.map_err(|_| {
            Error::ProtoSerialization("Failure to read SDP offer from signaling stream".to_string())
        })?;

        if offer_message.r#type != signaling_message::Type::SdpOffer as i32 {
            return Err(Error::Signaling("Expected SDP offer".to_string()));
        }

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

        pb_stream
            .lock()
            .await
            .write(answer_message)
            .await
            .map_err(|_| {
                Error::ProtoSerialization(
                    "Failure to write SDP answer to signaling stream".to_string(),
                )
            })?;

        self.wait_for_ice_gathering_complete().await?;

        let candidates = ice_candidates.borrow_mut().drain_candidates();
        Self::send_ice_candidates(&pb_stream, candidates).await?;
        Self::receive_ice_candidates(&pb_stream, connection).await?;

        self.wait_for_established_conn().await?;

        tracing::info!(
            "WebRTC connection established - Current state: {:?}",
            connection.connection_state()
        );
        
        // Clean up callbacks and close signaling stream. ice_callbacks is only used during the
        // signaling process so its not saved in `ConnectionCallbacks`, but dropped immediately
        drop(_ice_callback);
        drop(pb_stream);

        tracing::info!("Completed signaling.");
        Ok(Connection::new(rtc_conn, Some(callbacks)))
    }

    #[instrument(skip(stream), fields(initiator = true))]
    async fn signaling_as_initiator(
        &self,
        stream: SignalingStream<impl AsyncRead + AsyncWrite + Unpin + 'static>,
    ) -> Result<Connection, Error> {
        tracing::info!("Starting WebRTC signaling as initiator");
        
        let rtc_conn = RtcPeerConnection::new("sha-256".to_string(), self.config.stun_servers).await?;
        let connection = rtc_conn.inner();

        let pb_stream = Arc::new(Mutex::new(stream));
        let callbacks = self.setup_peer_connection_state_callbacks(connection);
        let (ice_candidates, _ice_callback) = self.setup_ice_candidate_collection(connection);

        // Create a data channel to ensure ICE information is shared in the SDP
        tracing::trace!("Creating init data channel");
        let data_channel = connection.create_data_channel("init");

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
            data: offer_sdp.clone(),
        };

        pb_stream.lock().await.write(message).await.map_err(|_| {
            Error::ProtoSerialization("Failure to write SDP offer to signaling stream".to_string())
        })?;

        // Read SDP answer
        let answer_message = pb_stream.lock().await.read().await.map_err(|_| {
            Error::ProtoSerialization(
                "Failure to read SDP answer from signaling stream".to_string(),
            )
        })?;

        if answer_message.r#type != signaling_message::Type::SdpAnswer as i32 {
            return Err(Error::Signaling("Expected SDP answer".to_string()));
        }

        let answer_init = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        answer_init.set_sdp(&answer_message.data);

        // Set answer as remote description
        JsFuture::from(connection.set_remote_description(&answer_init))
            .await
            .map_err(|_| Error::Js("Could not set remote description".to_string()))?;

        self.wait_for_ice_gathering_complete().await?;

        let candidates = ice_candidates.borrow_mut().drain_candidates();
        Self::send_ice_candidates(&pb_stream, candidates).await?;
        Self::receive_ice_candidates(&pb_stream, connection).await?;

        self.wait_for_established_conn().await?;

        tracing::info!(
            "WebRTC connection established - Current state: {:?}",
            connection.connection_state()
        );

        // Clean up callbacks, close data channel, and close signaling stream
        data_channel.close();

        // Clean up callbacks and close signaling stream. ice_callbacks is only used during the
        // signaling process so its not saved in `ConnectionCallbacks`, but dropped immediately
        drop(_ice_callback);
        drop(pb_stream);

        tracing::info!("Signaling complete.");
        Ok(Connection::new(rtc_conn, Some(callbacks)))
    }
}
