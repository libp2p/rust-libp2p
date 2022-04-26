// Copyright 2022 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use futures::channel::oneshot;
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use log::{debug, error, trace};
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::dtls_transport::dtls_role::DTLSRole;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc_data::data_channel::DataChannel as DetachedDataChannel;
use webrtc_ice::udp_mux::UDPMux;

use std::sync::Arc;
use std::time::Duration;

use crate::connection::Connection;
use crate::error::Error;
use crate::sdp;
use crate::transport;

pub struct WebRTCUpgrade {}

impl WebRTCUpgrade {
    pub async fn new(
        udp_mux: Arc<dyn UDPMux + Send + Sync>,
        config: RTCConfiguration,
        addr: Multiaddr,
    ) -> Result<Connection<'static>, Error> {
        trace!("upgrading {}", addr);

        let socket_addr = transport::multiaddr_to_socketaddr(&addr)
            .ok_or_else(|| Error::InvalidMultiaddr(addr.clone()))?;
        let fingerprint = transport::fingerprint_of_first_certificate(&config);

        let mut se = transport::build_setting_engine(udp_mux, &socket_addr, &fingerprint);
        {
            // Act as a lite ICE (ICE which does not send additional candidates).
            se.set_lite(true);
            // Act as a DTLS server (one which waits for a connection).
            //
            // NOTE: removing this seems to break DTLS setup (both sides send `ClientHello` messages,
            // but none end up responding).
            se.set_answering_dtls_role(DTLSRole::Server)
                .map_err(Error::WebRTC)?;
        }
        let api = APIBuilder::new().with_setting_engine(se).build();
        let peer_connection = api.new_peer_connection(config).await?;

        // Create a datachannel with label 'data'.
        let data_channel = peer_connection
            .create_data_channel(
                "data",
                Some(RTCDataChannelInit {
                    negotiated: Some(true),
                    id: Some(1),
                    ordered: None,
                    max_retransmits: None,
                    max_packet_life_time: None,
                    protocol: None,
                }),
            )
            .await?;

        let (data_channel_rx, data_channel_tx) = oneshot::channel::<Arc<DetachedDataChannel>>();

        // Wait until the data channel is opened and detach it.
        let d = Arc::clone(&data_channel);
        data_channel
            .on_open(Box::new(move || {
                debug!("Data channel '{}'-'{}' open.", d.label(), d.id());

                let d2 = Arc::clone(&d);
                Box::pin(async move {
                    match d2.detach().await {
                        Ok(detached) => {
                            if let Err(_) = data_channel_rx.send(detached) {
                                error!("data_channel_tx dropped");
                            }
                        },
                        Err(e) => {
                            error!("Can't detach data channel: {}", e);
                        },
                    };
                })
            }))
            .await;

        // Set the remote description to the predefined SDP.
        let fingerprint = match addr.iter().last() {
            Some(Protocol::XWebRTC(f)) => f,
            _ => {
                debug!("{} is not a WebRTC multiaddr", addr);
                return Err(Error::InvalidMultiaddr(addr));
            },
        };
        let client_session_description = transport::render_description(
            sdp::CLIENT_SESSION_DESCRIPTION,
            socket_addr,
            &transport::fingerprint_to_string(&fingerprint),
        );
        debug!("OFFER: {:?}", client_session_description);
        let sdp = RTCSessionDescription::offer(client_session_description).unwrap();
        peer_connection.set_remote_description(sdp).await?;

        let answer = peer_connection.create_answer(None).await?;
        // Set the local description and start UDP listeners
        // Note: this will start the gathering of ICE candidates
        debug!("ANSWER: {:?}", answer.sdp);
        peer_connection.set_local_description(answer).await?;

        // wait until data channel is opened and ready to use
        match tokio_crate::time::timeout(Duration::from_secs(10), data_channel_tx).await {
            Ok(Ok(dc)) => Ok(Connection::new(peer_connection, dc)),
            Ok(Err(e)) => Err(Error::InternalError(e.to_string())),
            Err(_) => Err(Error::InternalError(
                "data channel opening took longer than 10 seconds (see logs)".into(),
            )),
        }
    }
}
