
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{RtcSdpType, RtcSessionDescriptionInit};
use crate::browser::pb::{SignalingMessage, signaling_message};
use crate::{
    error::Error,
   
    stream::Stream,
};

use super::ProtobufStream;

/// Protocol ID for the WebRTC signaling protocol.
pub const SIGNALING_PROTOCOL_ID: &str = "/webrtc-signaling/0.0.1";

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
#[derive(Clone)]
pub(crate) struct SignalingProtocol;

impl Signaling for SignalingProtocol {
    async fn perform_signaling(
        &self,
        connection: web_sys::RtcPeerConnection,
        stream: Stream,
        is_initiator: bool,
    ) -> Result<(), Error> {
        let mut pb_stream = ProtobufStream::<SignalingMessage>::new(stream);

        if is_initiator {
            // Performs signaling protocol as the role of the initiator (create offer and receives answer)

            // Create a data channel to ensure ICE information is shared in the SDP
            let data_channel = connection.create_data_channel("init");

            // Create offer and set local description
            let offer = JsFuture::from(connection.create_offer()).await?;
            let offer_sdp = js_sys::Reflect::get(&offer, &JsValue::from_str("sdp"))?
                .as_string()
                .ok_or_else(|| Error::Js("Could not extract SDP from offer".to_string()))?;

            let offer_init = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
            offer_init.set_sdp(&offer_sdp);

            JsFuture::from(connection.set_local_description(&offer_init)).await;

            // Send the SDP offer
            let message = SignalingMessage {
                r#type: signaling_message::Type::SdpOffer as i32,
                data: offer_sdp,
            };
            pb_stream.write(message);

            // Receive SDP answer, create answer and set remote description
            let answer_message = pb_stream.read().await?;
            let answer_init = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
            answer_init.set_sdp(&answer_message.data);

            JsFuture::from(connection.set_remote_description(&answer_init)).await?;

            data_channel.close();
        } else {
            // Performs signaling protocol as the role of the remote
            
            // Answer role (receives offer and sends answer)
            // Receive SDP offer
            let offer_message = pb_stream.read().await?;
           
            // Set remote description
            let offer_init = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
            offer_init.set_sdp(&offer_message.data);

            JsFuture::from(connection.set_remote_description(&offer_init)).await?;

            // Create and set local description
            let answer = JsFuture::from(connection.create_answer()).await?;
            let answer_sdp = js_sys::Reflect::get(&answer, &JsValue::from_str("sdp"))?
                .as_string()
                .ok_or_else(|| {
                    Error::Js("Could not extract SDP from answer".to_string())
                })?;

            let answer_init = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
            answer_init.set_sdp(&answer_sdp);

            JsFuture::from(connection.set_local_description(&answer_init)).await?;

            // Send SDP answer
            let answer_message = SignalingMessage {
                r#type: signaling_message::Type::SdpAnswer as i32,
                data: answer_sdp,
            };
            
            pb_stream.write(answer_message).await?;
        }

        Ok(())
    }
}
