use std::{cell::RefCell, rc::Rc};

use async_trait::async_trait;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, StreamExt, channel::mpsc};
use tracing::{info, instrument};
use wasm_bindgen::{JsCast, JsValue, prelude::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    RtcIceCandidate, RtcIceCandidateInit, RtcIceConnectionState, RtcIceGatheringState,
    RtcPeerConnectionIceEvent, RtcPeerConnectionState, RtcSdpType, RtcSessionDescriptionInit,
    RtcSignalingState,
};

use crate::{
    Connection,
    browser::{
        SignalingConfig, SignalingStream,
        protocol::proto::signaling::{SignalingMessage, mod_SignalingMessage::Type},
        stream::{read_message, write_message},
    },
    connection::RtcPeerConnection,
    error::Error,
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
pub(crate) struct ConnectionCallbacks {
    _ice_connection_callback: Closure<dyn FnMut(web_sys::Event)>,
    _ice_gathering_callback: Closure<dyn FnMut(web_sys::Event)>,
    _peer_connection_callback: Closure<dyn FnMut(web_sys::Event)>,
    _signaling_callback: Closure<dyn FnMut(web_sys::Event)>,
}

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

#[async_trait(?Send)]
pub trait Signaling {
    async fn signaling_as_initiator(
        &self,
        stream: SignalingStream<impl AsyncRead + AsyncWrite + Unpin + 'static>,
    ) -> Result<Connection, Error>;

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

fn peer_connection_from_event(event: &web_sys::Event) -> Option<web_sys::RtcPeerConnection> {
    event
        .target()
        .and_then(|target| target.dyn_into::<web_sys::RtcPeerConnection>().ok())
}

impl ProtocolHandler {
    fn setup_peer_connection_state_callbacks(
        &self,
        connection: &web_sys::RtcPeerConnection,
    ) -> ConnectionCallbacks {
        tracing::trace!("Setting up peer connection state callbacks");

        let states = self.states.clone();
        let ice_connection_callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if let Some(pc) = peer_connection_from_event(&event) {
                let state = safe_ice_connection_state_from_js(pc.ice_connection_state().into());
                tracing::debug!("ICE connection state changed to: {:?}", state);
                states.borrow_mut().ice_connection = state;
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        connection
            .set_oniceconnectionstatechange(Some(ice_connection_callback.as_ref().unchecked_ref()));

        let states = self.states.clone();
        let ice_gathering_callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if let Some(pc) = peer_connection_from_event(&event) {
                let state = safe_ice_gathering_state_from_js(pc.ice_gathering_state().into());
                tracing::debug!("ICE gathering state changed to: {:?}", state);
                states.borrow_mut().ice_gathering = state;
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        connection
            .set_onicegatheringstatechange(Some(ice_gathering_callback.as_ref().unchecked_ref()));

        let states = self.states.clone();
        let peer_connection_callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if let Some(pc) = peer_connection_from_event(&event) {
                let state = safe_peer_connection_state_from_js(pc.connection_state().into());
                tracing::debug!("Peer connection state changed to: {:?}", state);
                states.borrow_mut().peer_connection = state;
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        connection
            .set_onconnectionstatechange(Some(peer_connection_callback.as_ref().unchecked_ref()));

        let states = self.states.clone();
        let signaling_callback = Closure::wrap(Box::new(move |event: web_sys::Event| {
            if let Some(pc) = peer_connection_from_event(&event) {
                let state = safe_signaling_state_from_js(pc.signaling_state().into());
                tracing::debug!("Signaling state changed to: {:?}", state);
                states.borrow_mut().signaling = state;
            }
        }) as Box<dyn FnMut(web_sys::Event)>);
        connection.set_onsignalingstatechange(Some(signaling_callback.as_ref().unchecked_ref()));

        ConnectionCallbacks {
            _ice_connection_callback: ice_connection_callback,
            _ice_gathering_callback: ice_gathering_callback,
            _peer_connection_callback: peer_connection_callback,
            _signaling_callback: signaling_callback,
        }
    }

    /// Wires the `onicecandidate` event so that each newly discovered candidate is
    /// pushed onto the supplied channel for trickle delivery to the remote.
    /// `None` is sent when gathering is complete.
    fn setup_trickle_ice_callback(
        connection: &web_sys::RtcPeerConnection,
        mut tx: mpsc::Sender<Option<RtcIceCandidate>>,
    ) -> Closure<dyn FnMut(RtcPeerConnectionIceEvent)> {
        let cb = Closure::wrap(Box::new(move |event: RtcPeerConnectionIceEvent| {
            if let Err(e) = tx.try_send(event.candidate()) {
                tracing::debug!("Could not enqueue ICE candidate: {:?}", e);
            }
        }) as Box<dyn FnMut(RtcPeerConnectionIceEvent)>);

        connection.set_onicecandidate(Some(cb.as_ref().unchecked_ref()));
        cb
    }

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
                RtcPeerConnectionState::Connected => return Ok(()),
                RtcPeerConnectionState::Failed => {
                    return Err(Error::Connection("Peer connection failed".to_string()));
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
                        return Err(Error::Connection("Connection timeout".to_string()));
                    }

                    gloo_timers::future::sleep(
                        self.config.connection_establishment_delay_in_millis,
                    )
                    .await;
                }
            }
        }
    }

    /// Parse ICE candidate from the JSON payload of a [`SignalingMessage`].
    fn parse_ice_candidate(data: &str) -> Option<RtcIceCandidateInit> {
        let candidate_json = serde_json::from_str::<serde_json::Value>(data).ok()?;
        let candidate_str = candidate_json.get("candidate")?.as_str()?;

        let candidate_init = RtcIceCandidateInit::new(candidate_str);

        if let Some(sdp_mid) = candidate_json.get("sdpMid").and_then(|v| v.as_str()) {
            candidate_init.set_sdp_mid(Some(sdp_mid));
        }

        if let Some(sdp_m_line_index) = candidate_json.get("sdpMLineIndex").and_then(|v| v.as_u64())
        {
            candidate_init.set_sdp_m_line_index(Some(sdp_m_line_index as u16));
        }

        Some(candidate_init)
    }
}

/// Spawns a local task that reads ICE candidates from the remote until EOF and
/// feeds them to [`web_sys::RtcPeerConnection::add_ice_candidate_with_opt_rtc_ice_candidate_init`].
///
/// The task is detached: it terminates naturally when the remote closes its
/// half of the stream.
fn spawn_remote_candidate_reader<R>(mut reader: R, connection: web_sys::RtcPeerConnection)
where
    R: AsyncRead + Unpin + 'static,
{
    wasm_bindgen_futures::spawn_local(async move {
        loop {
            let message = match read_message(&mut reader).await {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    tracing::debug!("Remote closed signaling stream");
                    return;
                }
                Err(e) => {
                    tracing::debug!("Signaling stream read error: {}", e);
                    return;
                }
            };

            if message.type_pb != Some(Type::ICE_CANDIDATE) {
                tracing::warn!(
                    "Ignoring non-ICE message during trickle phase: {:?}",
                    message.type_pb
                );
                continue;
            }

            let data = message.data.unwrap_or_default();
            if data.is_empty() {
                tracing::debug!("Remote signaled end-of-candidates");
                continue;
            }

            let Some(candidate_init) = ProtocolHandler::parse_ice_candidate(&data) else {
                tracing::debug!("Skipping un-parseable remote ICE candidate: {}", data);
                continue;
            };

            if let Err(e) = JsFuture::from(
                connection.add_ice_candidate_with_opt_rtc_ice_candidate_init(Some(&candidate_init)),
            )
            .await
            {
                tracing::warn!("Failed to add remote ICE candidate: {:?}", e);
            }
        }
    });
}

/// Forwards locally discovered ICE candidates over the stream as they arrive.
/// Returns when gathering completes (channel yields `None` or the sentinel `None`).
async fn run_local_candidate_writer<W>(
    mut writer: W,
    mut local_candidates: mpsc::Receiver<Option<RtcIceCandidate>>,
) -> W
where
    W: AsyncWrite + Unpin,
{
    while let Some(maybe_candidate) = local_candidates.next().await {
        let Some(candidate) = maybe_candidate else {
            tracing::debug!("Local ICE gathering complete");
            break;
        };

        let candidate_json = candidate.to_json();
        let data = js_sys::JSON::stringify(&candidate_json)
            .ok()
            .and_then(|s| s.as_string())
            .unwrap_or_default();

        let message = SignalingMessage {
            type_pb: Some(Type::ICE_CANDIDATE),
            data: Some(data),
        };

        if let Err(e) = write_message(&mut writer, message).await {
            tracing::warn!("Failed to write local ICE candidate: {}", e);
            break;
        }
    }

    writer
}

#[async_trait(?Send)]
impl Signaling for ProtocolHandler {
    #[instrument(skip(stream), fields(initiator = false))]
    async fn signaling_as_responder(
        &self,
        stream: SignalingStream<impl AsyncRead + AsyncWrite + Unpin + 'static>,
    ) -> Result<Connection, Error> {
        tracing::info!("Starting WebRTC signaling as responder");

        let rtc_conn =
            RtcPeerConnection::new("sha-256".to_string(), self.config.stun_servers.clone()).await?;
        let connection = rtc_conn.inner().clone();

        let callbacks = self.setup_peer_connection_state_callbacks(&connection);
        // 64 slots comfortably exceeds the typical number of host/srflx ICE
        // candidates a single connection produces; if the channel ever fills,
        // the dropped candidate is logged and the rest still flow.
        let (cand_tx, cand_rx) = mpsc::channel(64);
        let _ice_callback = Self::setup_trickle_ice_callback(&connection, cand_tx);

        let inner = stream.into_inner();
        let (mut reader, mut writer) = inner.split();

        let offer_message = read_message(&mut reader)
            .await
            .map_err(|e| Error::Js(format!("Failed to read SDP offer: {e}")))?;

        if offer_message.type_pb != Some(Type::SDP_OFFER) {
            return Err(Error::Connection("Expected SDP offer".to_string()));
        }
        let offer_sdp = offer_message
            .data
            .ok_or_else(|| Error::Connection("SDP offer missing data".to_string()))?;

        let offer_init = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
        offer_init.set_sdp(&offer_sdp);
        JsFuture::from(connection.set_remote_description(&offer_init))
            .await
            .map_err(|_| Error::Js("Could not set remote description".to_string()))?;

        let answer = JsFuture::from(connection.create_answer()).await?;
        let answer_sdp = js_sys::Reflect::get(&answer, &JsValue::from_str("sdp"))?
            .as_string()
            .ok_or_else(|| Error::Js("Could not extract SDP from answer".to_string()))?;

        let answer_init = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        answer_init.set_sdp(&answer_sdp);
        JsFuture::from(connection.set_local_description(&answer_init))
            .await
            .map_err(|_| Error::Js("Could not set local description".to_string()))?;

        write_message(
            &mut writer,
            SignalingMessage {
                type_pb: Some(Type::SDP_ANSWER),
                data: Some(answer_sdp),
            },
        )
        .await
        .map_err(|e| Error::Js(format!("Failed to write SDP answer: {e}")))?;

        // read remote candidates in a detached task, write local
        // candidates from the main task, wait for the peer connection to come up.
        spawn_remote_candidate_reader(reader, connection.clone());
        let writer_task = run_local_candidate_writer(writer, cand_rx);
        let establish = self.wait_for_established_conn();
        let (mut writer, established) = futures::join!(writer_task, establish);
        established?;

        let _ = writer.close().await;

        tracing::info!(
            "WebRTC connection established - Current state: {:?}",
            connection.connection_state()
        );

        drop(_ice_callback);

        Ok(Connection::new(rtc_conn, Some(callbacks)))
    }

    #[instrument(skip(stream), fields(initiator = true))]
    async fn signaling_as_initiator(
        &self,
        stream: SignalingStream<impl AsyncRead + AsyncWrite + Unpin + 'static>,
    ) -> Result<Connection, Error> {
        tracing::info!("Starting WebRTC signaling as initiator");

        let rtc_conn =
            RtcPeerConnection::new("sha-256".to_string(), self.config.stun_servers.clone()).await?;
        let connection = rtc_conn.inner().clone();

        let callbacks = self.setup_peer_connection_state_callbacks(&connection);
        // 64 slots comfortably exceeds the typical number of host/srflx ICE
        // candidates a single connection produces; if the channel ever fills,
        // the dropped candidate is logged and the rest still flow.
        let (cand_tx, cand_rx) = mpsc::channel(64);
        let _ice_callback = Self::setup_trickle_ice_callback(&connection, cand_tx);

        // The init data channel ensures ICE info is included in the SDP offer.
        // only the initiator creates this and only the
        // initiator closes it once the connection is established.
        tracing::trace!("Creating init data channel");
        let data_channel = connection.create_data_channel("init");

        let offer = JsFuture::from(connection.create_offer()).await?;
        let offer_sdp = js_sys::Reflect::get(&offer, &JsValue::from_str("sdp"))?
            .as_string()
            .ok_or_else(|| Error::Js("Could not extract SDP from offer".to_string()))?;

        let offer_init = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
        offer_init.set_sdp(&offer_sdp);
        JsFuture::from(connection.set_local_description(&offer_init))
            .await
            .map_err(|_| Error::Js("Could not set local description".to_string()))?;

        let inner = stream.into_inner();
        let (mut reader, mut writer) = inner.split();

        write_message(
            &mut writer,
            SignalingMessage {
                type_pb: Some(Type::SDP_OFFER),
                data: Some(offer_sdp),
            },
        )
        .await
        .map_err(|e| Error::Js(format!("Failed to write SDP offer: {e}")))?;

        let answer_message = read_message(&mut reader)
            .await
            .map_err(|e| Error::Js(format!("Failed to read SDP answer: {e}")))?;

        if answer_message.type_pb != Some(Type::SDP_ANSWER) {
            return Err(Error::Connection("Expected SDP answer".to_string()));
        }
        let answer_sdp = answer_message
            .data
            .ok_or_else(|| Error::Connection("SDP answer missing data".to_string()))?;

        let answer_init = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        answer_init.set_sdp(&answer_sdp);
        JsFuture::from(connection.set_remote_description(&answer_init))
            .await
            .map_err(|_| Error::Js("Could not set remote description".to_string()))?;

        spawn_remote_candidate_reader(reader, connection.clone());
        let writer_task = run_local_candidate_writer(writer, cand_rx);
        let establish = self.wait_for_established_conn();
        let (mut writer, established) = futures::join!(writer_task, establish);
        established?;

        // close the init data channel and the signaling stream
        // once the connection is established.
        data_channel.close();
        let _ = writer.close().await;

        tracing::info!(
            "WebRTC connection established - Current state: {:?}",
            connection.connection_state()
        );

        drop(_ice_callback);

        Ok(Connection::new(rtc_conn, Some(callbacks)))
    }
}
