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

pub mod noise;

use crate::connection::PollDataChannel;
use crate::error::Error;
use crate::fingerprint::Fingerprint;
use crate::Connection;
use futures::{channel::oneshot, prelude::*, select};
use futures_timer::Delay;
use libp2p_core::{identity, PeerId};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::APIBuilder;
use webrtc::data::data_channel::DataChannel;
use webrtc::data_channel::data_channel_init::RTCDataChannelInit;
use webrtc::dtls_transport::dtls_role::DTLSRole;
use webrtc::ice::network_type::NetworkType;
use webrtc::ice::udp_mux::UDPMux;
use webrtc::ice::udp_network::UDPNetwork;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

/// Creates a new outbound WebRTC connection.
pub async fn outbound(
    addr: SocketAddr,
    config: RTCConfiguration,
    udp_mux: Arc<dyn UDPMux + Send + Sync>,
    our_fingerprint: Fingerprint,
    remote_fingerprint: Fingerprint,
    id_keys: identity::Keypair,
    expected_peer_id: PeerId,
) -> Result<(PeerId, Connection), Error> {
    let peer_connection = new_outbound_connection(addr, config, udp_mux).await?;

    // 1. OFFER
    let offer = peer_connection.create_offer(None).await?;
    log::debug!("OFFER: {:?}", offer.sdp);
    peer_connection.set_local_description(offer).await?;

    // 2. ANSWER
    // Set the remote description to the predefined SDP.
    let server_session_description =
        crate::sdp::render_server_session_description(addr, &remote_fingerprint);
    log::debug!("ANSWER: {:?}", server_session_description);
    let sdp = RTCSessionDescription::answer(server_session_description).unwrap();
    // NOTE: this will start the gathering of ICE candidates
    peer_connection.set_remote_description(sdp).await?;

    // Open a data channel to do Noise on top and verify the remote.
    let data_channel = create_initial_upgrade_data_channel(&peer_connection).await?;

    log::trace!("noise handshake with addr={}", addr);
    let peer_id = noise::outbound(
        id_keys,
        PollDataChannel::new(data_channel.clone()),
        our_fingerprint,
        remote_fingerprint,
    )
    .await?;

    log::trace!("verifying peer's identity addr={}", addr);
    if expected_peer_id != peer_id {
        return Err(Error::InvalidPeerID {
            expected: expected_peer_id,
            got: peer_id,
        });
    }

    // Close the initial data channel after noise handshake is done.
    data_channel
        .close()
        .await
        .map_err(|e| Error::WebRTC(webrtc::Error::Data(e)))?;

    Ok((peer_id, Connection::new(peer_connection).await))
}

/// Creates a new inbound WebRTC connection.
pub async fn inbound(
    addr: SocketAddr,
    config: RTCConfiguration,
    udp_mux: Arc<dyn UDPMux + Send + Sync>,
    our_fingerprint: Fingerprint,
    remote_ufrag: String,
    id_keys: identity::Keypair,
) -> Result<(PeerId, Connection), Error> {
    log::trace!("upgrading addr={} (ufrag={})", addr, remote_ufrag);

    let ufrag = our_fingerprint.to_ufrag();

    let peer_connection = new_inbound_connection(addr, config, udp_mux, &ufrag).await?;

    let client_session_description =
        crate::sdp::render_client_session_description(addr, &remote_ufrag);
    log::debug!("OFFER: {:?}", client_session_description);
    let sdp = RTCSessionDescription::offer(client_session_description).unwrap();
    peer_connection.set_remote_description(sdp).await?;

    let answer = peer_connection.create_answer(None).await?;
    // Set the local description and start UDP listeners
    // Note: this will start the gathering of ICE candidates
    log::debug!("ANSWER: {:?}", answer.sdp);
    peer_connection.set_local_description(answer).await?;

    // Open a data channel to do Noise on top and verify the remote.
    let data_channel = create_initial_upgrade_data_channel(&peer_connection).await?;

    log::trace!("noise handshake with addr={} (ufrag={})", addr, ufrag);
    let remote_fingerprint = get_remote_fingerprint(&peer_connection).await;
    let peer_id = noise::inbound(
        id_keys,
        PollDataChannel::new(data_channel.clone()),
        our_fingerprint,
        remote_fingerprint,
    )
    .await?;

    // Close the initial data channel after noise handshake is done.
    data_channel
        .close()
        .await
        .map_err(|e| Error::WebRTC(webrtc::Error::Data(e)))?;

    Ok((peer_id, Connection::new(peer_connection).await))
}

async fn new_outbound_connection(
    addr: SocketAddr,
    config: RTCConfiguration,
    udp_mux: Arc<dyn UDPMux + Send + Sync>,
) -> Result<RTCPeerConnection, Error> {
    let ufrag = random_ufrag();
    let se = setting_engine(udp_mux, &ufrag, addr);

    let connection = APIBuilder::new()
        .with_setting_engine(se)
        .build()
        .new_peer_connection(config)
        .await?;

    Ok(connection)
}

async fn new_inbound_connection(
    addr: SocketAddr,
    config: RTCConfiguration,
    udp_mux: Arc<dyn UDPMux + Send + Sync>,
    ufrag: &str,
) -> Result<RTCPeerConnection, Error> {
    let mut se = setting_engine(udp_mux, &ufrag, addr);
    {
        se.set_lite(true);
        se.disable_certificate_fingerprint_verification(true);
        // Act as a DTLS server (one which waits for a connection).
        //
        // NOTE: removing this seems to break DTLS setup (both sides send `ClientHello` messages,
        // but none end up responding).
        se.set_answering_dtls_role(DTLSRole::Server)?;
    }

    let connection = APIBuilder::new()
        .with_setting_engine(se)
        .build()
        .new_peer_connection(config)
        .await?;

    Ok(connection)
}

fn random_ufrag() -> String {
    // TODO: at least 128 bit of entropy

    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect()
}

fn setting_engine(
    udp_mux: Arc<dyn UDPMux + Send + Sync>,
    ufrag: &str,
    addr: SocketAddr,
) -> SettingEngine {
    let mut se = SettingEngine::default();

    // Set both ICE user and password to our fingerprint because that's what the client is
    // expecting..
    se.set_ice_credentials(ufrag.to_owned(), ufrag.to_owned());

    se.set_udp_network(UDPNetwork::Muxed(udp_mux.clone()));

    // Allow detaching data channels.
    se.detach_data_channels();

    // Set the desired network type.
    //
    // NOTE: if not set, a [`webrtc_ice::agent::Agent`] might pick a wrong local candidate
    // (e.g. IPv6 `[::1]` while dialing an IPv4 `10.11.12.13`).
    let network_type = match addr {
        SocketAddr::V4(_) => NetworkType::Udp4,
        SocketAddr::V6(_) => NetworkType::Udp6,
    };
    se.set_network_types(vec![network_type]);

    se
}

/// Returns the SHA-256 fingerprint of the remote.
async fn get_remote_fingerprint(conn: &RTCPeerConnection) -> Fingerprint {
    let cert_bytes = conn.sctp().transport().get_remote_certificate().await;

    Fingerprint::from_certificate(&cert_bytes)
}

async fn create_initial_upgrade_data_channel(
    conn: &RTCPeerConnection,
) -> Result<Arc<DataChannel>, Error> {
    // Open a data channel to do Noise on top and verify the remote.
    let data_channel = conn
        .create_data_channel(
            "data",
            Some(RTCDataChannelInit {
                negotiated: Some(0),
                ..RTCDataChannelInit::default()
            }),
        )
        .await?;

    let (tx, mut rx) = oneshot::channel::<Arc<DataChannel>>();

    // Wait until the data channel is opened and detach it.
    crate::connection::register_data_channel_open_handler(data_channel, tx).await;
    select! {
        res = rx => match res {
            Ok(detached) => Ok(detached),
            Err(e) => Err(Error::Internal(e.to_string())),
        },
        _ = Delay::new(Duration::from_secs(10)).fuse() => Err(Error::Internal(
            "data channel opening took longer than 10 seconds (see logs)".into(),
        ))
    }
}
