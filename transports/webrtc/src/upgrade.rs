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

use futures::io::{AsyncReadExt, AsyncWriteExt};
use futures::{channel::oneshot, future, select, FutureExt, TryFutureExt};
use futures_timer::Delay;
use libp2p_core::identity;
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    PeerId,
};
use libp2p_core::{InboundUpgrade, UpgradeInfo};
use libp2p_noise::{Keypair, NoiseConfig, NoiseError, RemoteIdentity, X25519Spec};
use log::{debug, trace};
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
use crate::connection::PollDataChannel;
use crate::error::Error;
use crate::sdp;
use crate::transport;

pub async fn webrtc(
    udp_mux: Arc<dyn UDPMux + Send + Sync>,
    config: RTCConfiguration,
    addr: Multiaddr,
    id_keys: identity::Keypair,
) -> Result<(PeerId, Connection), Error> {
    trace!("upgrading {}", addr);

    let socket_addr = transport::multiaddr_to_socketaddr(&addr)
        .ok_or_else(|| Error::InvalidMultiaddr(addr.clone()))?;
    let our_fingerprint = transport::fingerprint_of_first_certificate(&config);

    let mut se = transport::build_setting_engine(udp_mux, &socket_addr, &our_fingerprint);
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

    // Set the remote description to the predefined SDP.
    let remote_fingerprint = if let Some(Protocol::XWebRTC(f)) = addr.iter().last() {
        transport::fingerprint_to_string(&f)
    } else {
        debug!("{} is not a WebRTC multiaddr", addr);
        return Err(Error::InvalidMultiaddr(addr));
    };
    let client_session_description = transport::render_description(
        sdp::CLIENT_SESSION_DESCRIPTION,
        socket_addr,
        &remote_fingerprint,
    );
    debug!("OFFER: {:?}", client_session_description);
    let sdp = RTCSessionDescription::offer(client_session_description).unwrap();
    peer_connection.set_remote_description(sdp).await?;

    let answer = peer_connection.create_answer(None).await?;
    // Set the local description and start UDP listeners
    // Note: this will start the gathering of ICE candidates
    debug!("ANSWER: {:?}", answer.sdp);
    peer_connection.set_local_description(answer).await?;

    // Create a datachannel with label 'data'.
    let data_channel = peer_connection
        .create_data_channel(
            "data",
            Some(RTCDataChannelInit {
                negotiated: Some(true),
                id: Some(1),
                ..RTCDataChannelInit::default()
            }),
        )
        .await?;

    let (tx, mut rx) = oneshot::channel::<Arc<DetachedDataChannel>>();

    // Wait until the data channel is opened and detach it.
    // Wait until the data channel is opened and detach it.
    crate::connection::register_data_channel_open_handler(data_channel, tx).await;

    // Wait until data channel is opened and ready to use
    let detached = select! {
        res = rx => match res {
            Ok(detached) => detached,
            Err(e) => return Err(Error::InternalError(e.to_string())),
        },
        _ = Delay::new(Duration::from_secs(10)).fuse() => return Err(Error::InternalError(
            "data channel opening took longer than 10 seconds (see logs)".into(),
        ))
    };

    trace!("noise handshake with {}", addr);
    let dh_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&id_keys)
        .unwrap();
    let noise = NoiseConfig::xx(dh_keys);
    let info = noise.protocol_info().next().unwrap();
    let (peer_id, mut noise_io) = noise
        .upgrade_inbound(PollDataChannel::new(detached.clone()), info)
        .and_then(|(remote, io)| match remote {
            RemoteIdentity::IdentityKey(pk) => future::ok((pk.to_peer_id(), io)),
            _ => future::err(NoiseError::AuthenticationFailed),
        })
        .await
        .map_err(Error::Noise)?;

    // Exchange TLS certificate fingerprints to prevent MiM attacks.
    trace!("exchanging TLS certificate fingerprints with {}", addr);
    let n = noise_io.write(&our_fingerprint.into_bytes()).await?;
    noise_io.flush().await?;
    let mut buf = vec![0; n]; // ASSERT: fingerprint's format is the same.
    noise_io.read_exact(buf.as_mut_slice()).await?;
    let fingerprint_from_noise =
        String::from_utf8(buf).map_err(|_| Error::Noise(NoiseError::AuthenticationFailed))?;
    if fingerprint_from_noise != remote_fingerprint {
        return Err(Error::InvalidFingerprint {
            expected: remote_fingerprint,
            got: fingerprint_from_noise,
        });
    }

    // Close the initial data channel after noise handshake is done.
    // https://github.com/webrtc-rs/sctp/pull/14
    // detached
    //     .close()
    //     .await
    //     .map_err(|e| Error::WebRTC(e.into()))?;

    Ok((peer_id, Connection::new(peer_connection).await))
}
