use std::{cell::RefCell, rc::Rc, sync::mpsc};

use wasm_bindgen::{prelude::Closure, JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    RtcIceCandidate, RtcIceCandidateInit, RtcIceConnectionState, RtcIceGatheringState,
    RtcPeerConnectionIceEvent, RtcPeerConnectionState, RtcSdpType, RtcSessionDescriptionInit,
    RtcSignalingState,
};

use super::ProtobufStream;
use crate::{
    browser::pb::{signaling_message, SignalingMessage},
    error::Error,
    stream::Stream,
};

/// Protocol ID for the WebRTC signaling protocol.
pub const SIGNALING_PROTOCOL_ID: &str = "/webrtc-signaling/0.0.1";

#[derive(Clone)]
struct ConnectionState {
    ice_connection: RtcIceConnectionState,
    ice_gathering: RtcIceGatheringState,
    signaling: RtcSignalingState,
    peer_connection: RtcPeerConnectionState,
}

/// A trait containing functionality required to perform WebRTC signaling.
pub trait Signaling {
    async fn perform_signaling(
        &self,
        connection: web_sys::RtcPeerConnection,
        stream: Stream,
        is_initiator: bool,
    ) -> Result<(), Error>;
}

/// Implementation of the WebRTC signaling protocol.
pub(crate) struct SignalingProtocol {
    states: Rc<RefCell<ConnectionState>>,
}

impl SignalingProtocol {
    pub(crate) fn new() -> Self {
        Self {
            states: Rc::new(RefCell::new(ConnectionState {
                ice_connection: RtcIceConnectionState::New,
                ice_gathering: RtcIceGatheringState::New,
                signaling: RtcSignalingState::Closed,
                peer_connection: RtcPeerConnectionState::Closed,
            })),
        }
    }
}

impl Signaling for SignalingProtocol {
    async fn perform_signaling(
        &self,
        connection: web_sys::RtcPeerConnection,
        stream: Stream,
        is_initiator: bool,
    ) -> Result<(), Error> {
        let mut pb_stream = ProtobufStream::<SignalingMessage>::new(stream);
        let (ice_candidate_sender, ice_candidate_receiver) = mpsc::channel::<RtcIceCandidate>();

        // Setup callbacks to handle various state changes for peer connections, ice states and
        // signaling state changes.

        // Set a callback to handle ice candidates. Candidates are sent to and handled by the ice
        // candidate channel receiver
        let set_onicecandidate_callback =
            Closure::wrap(Box::new(move |event: RtcPeerConnectionIceEvent| {
                let Some(candidate) = event.candidate() else {
                    return;
                };
                ice_candidate_sender.send(candidate).unwrap();
            }) as Box<dyn FnMut(RtcPeerConnectionIceEvent)>);
        connection.set_onicecandidate(Some(&set_onicecandidate_callback.as_ref().unchecked_ref()));

        // Set a callback to handle ice connection state changes
        let states = self.states.clone();
        let iceconnectionstatechange_callback =
            Closure::wrap(Box::new(move |state: RtcIceConnectionState| {
                states.borrow_mut().ice_connection = state;
            }) as Box<dyn FnMut(RtcIceConnectionState)>);

        connection.set_oniceconnectionstatechange(Some(
            &iceconnectionstatechange_callback.as_ref().unchecked_ref(),
        ));

        // Set a callback to handle ice gathering state changes
        let states = self.states.clone();
        let icegatheringstatechange_callback =
            Closure::wrap(Box::new(move |state: RtcIceGatheringState| {
                states.borrow_mut().ice_gathering = state;
            }) as Box<dyn FnMut(RtcIceGatheringState)>);

        connection.set_onicegatheringstatechange(Some(
            &icegatheringstatechange_callback.as_ref().unchecked_ref(),
        ));

        // Set a callback to handle peer connection state changes
        let states = self.states.clone();
        let peerconnectionstatechange_callback =
            Closure::wrap(Box::new(move |state: RtcPeerConnectionState| {
                states.borrow_mut().peer_connection = state;
            }) as Box<dyn FnMut(RtcPeerConnectionState)>);

        connection.set_onconnectionstatechange(Some(
            &peerconnectionstatechange_callback.as_ref().unchecked_ref(),
        ));

        // Set a callback to handle signaling state changes
        let states = self.states.clone();
        let signalingstatechange_callback =
            Closure::wrap(Box::new(move |state: RtcSignalingState| {
                states.borrow_mut().signaling = state;
            }) as Box<dyn FnMut(RtcSignalingState)>);

        connection.set_onsignalingstatechange(Some(
            &signalingstatechange_callback.as_ref().unchecked_ref(),
        ));

        if is_initiator {
            // Performs signaling protocol as the role of the initiator (create offer and receives
            // answer)

            // Create a data channel to ensure ICE information is shared in the SDP
            let data_channel = connection.create_data_channel("init");

            // Create offer and set local description
            let offer = JsFuture::from(connection.create_offer()).await?;
            let offer_sdp = js_sys::Reflect::get(&offer, &JsValue::from_str("sdp"))?
                .as_string()
                .ok_or_else(|| Error::Js("Could not extract SDP from offer".to_string()))?;

            let offer_init = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
            offer_init.set_sdp(&offer_sdp);

            JsFuture::from(connection.set_local_description(&offer_init))
                .await
                .map_err(|_| Error::Js("Could not set local description".to_string()))?;

            // Send the SDP offer
            let message = SignalingMessage {
                r#type: signaling_message::Type::SdpOffer as i32,
                data: offer_sdp,
            };
            pb_stream.write(message).await.map_err(|_| {
                Error::ProtoSerialization(
                    "Failure to write signaling message over stream".to_string(),
                )
            })?;

            // Receive SDP answer, create answer and set remote description
            let answer_message = pb_stream.read().await.map_err(|_| {
                Error::ProtoSerialization(
                    "Failure to read signaling message over stream".to_string(),
                )
            })?;
            let answer_init = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
            answer_init.set_sdp(&answer_message.data);

            JsFuture::from(connection.set_remote_description(&answer_init))
                .await
                .map_err(|_| Error::Js("Could not set remote description".to_string()))?;

            // Send ICE candidates to remote peer
            for candidate in ice_candidate_receiver.iter() {
                let message = SignalingMessage {
                    r#type: signaling_message::Type::IceCandidate as i32,
                    data: candidate.as_string().unwrap(),
                };
                pb_stream.write(message).await.map_err(|_| {
                    Error::ProtoSerialization(
                        "Failure to write signaling message over stream".to_string(),
                    )
                })?;
            }

            // Receive and add ice candidates from remote peer
            while let Some(message) = pb_stream.read().await.iter().next() {
                if message.r#type == signaling_message::Type::IceCandidate as i32 {
                    JsFuture::from(
                        connection.add_ice_candidate_with_opt_rtc_ice_candidate_init(Some(
                            &RtcIceCandidateInit::new(&message.data),
                        )),
                    )
                    .await
                    .map_err(|_| Error::Js("Error adding ice candidate".to_string()))?;
                }
            }

            data_channel.close();
        } else {
            // Performs signaling protocol as the role of the remote

            // Answer role (receives offer and sends answer)
            // Receive SDP offer
            let offer_message = pb_stream.read().await.map_err(|_| {
                Error::ProtoSerialization(
                    "Failure to read signaling message over stream".to_string(),
                )
            })?;

            // Set remote description
            let offer_init = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
            offer_init.set_sdp(&offer_message.data);

            JsFuture::from(connection.set_remote_description(&offer_init))
                .await
                .map_err(|_| Error::Js("Could not set remote description".to_string()))?;

            // Create and set local description
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

            pb_stream.write(answer_message).await.map_err(|_| {
                Error::ProtoSerialization(
                    "Failure to write signaling message over stream".to_string(),
                )
            })?;

            // Send ICE candidates to remote peer
            for candidate in ice_candidate_receiver.iter() {
                let message = SignalingMessage {
                    r#type: signaling_message::Type::IceCandidate as i32,
                    data: candidate.as_string().unwrap(),
                };
                pb_stream.write(message).await.map_err(|_| {
                    Error::ProtoSerialization(
                        "Failure to write signaling message over stream".to_string(),
                    )
                })?;
            }

            // Receive and add ice candidates from remote peer
            while let Some(message) = pb_stream.read().await.iter().next() {
                if message.r#type == signaling_message::Type::IceCandidate as i32 {
                    JsFuture::from(
                        connection.add_ice_candidate_with_opt_rtc_ice_candidate_init(Some(
                            &RtcIceCandidateInit::new(&message.data),
                        )),
                    )
                    .await
                    .map_err(|_| Error::Js("Error adding ice candidate".to_string()))?;
                }
            }
        }

        Ok(())
    }
}
