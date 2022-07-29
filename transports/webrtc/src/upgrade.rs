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

use futures::{
    future,
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    TryFutureExt,
};
use libp2p_core::{
    identity, PeerId, {InboundUpgrade, UpgradeInfo},
};
use libp2p_noise::{Keypair, NoiseConfig, NoiseError, RemoteIdentity, X25519Spec};
use log::{debug, trace};
use webrtc_ice::udp_mux::UDPMux;

use std::{net::SocketAddr, sync::Arc};

use crate::{
    connection::{Connection, PollDataChannel},
    error::Error,
    transport::WebRTCConfiguration,
    webrtc_connection::{Fingerprint, WebRTCConnection},
};

pub(crate) async fn webrtc(
    udp_mux: Arc<dyn UDPMux + Send + Sync>,
    config: WebRTCConfiguration,
    socket_addr: SocketAddr,
    ufrag: String,
    id_keys: identity::Keypair,
) -> Result<(PeerId, Connection), Error> {
    trace!("upgrading addr={} (ufrag={})", socket_addr, ufrag);

    let our_fingerprint = Fingerprint::new_sha256(config.fingerprint_of_first_certificate());

    let conn = WebRTCConnection::accept(
        socket_addr,
        config.into_inner(),
        udp_mux,
        &our_fingerprint,
        &ufrag,
    )
    .await?;

    // Open a data channel to do Noise on top and verify the remote.
    // NOTE: channel is already negotiated by the client
    let data_channel = conn.create_initial_upgrade_data_channel(Some(true)).await?;

    trace!(
        "noise handshake with addr={} (ufrag={})",
        socket_addr,
        ufrag
    );
    let remote_fingerprint = {
        let f = conn.get_remote_fingerprint().await;
        Fingerprint::from(f.iter())
    };
    let peer_id = perform_noise_handshake(
        id_keys,
        PollDataChannel::new(data_channel.clone()),
        our_fingerprint.as_ref(),
        remote_fingerprint.as_ref(),
    )
    .await?;

    // Close the initial data channel after noise handshake is done.
    data_channel
        .close()
        .await
        .map_err(|e| Error::WebRTC(webrtc::Error::Data(e)))?;

    let mut c = Connection::new(conn.into_inner()).await;
    // TODO: default buffer size is too small to fit some messages. Possibly remove once
    // https://github.com/webrtc-rs/sctp/issues/28 is fixed.
    c.set_data_channels_read_buf_capacity(8192 * 10);

    Ok((peer_id, c))
}

async fn perform_noise_handshake<T>(
    id_keys: identity::Keypair,
    poll_data_channel: T,
    our_fingerprint: &str,
    remote_fingerprint: &str,
) -> Result<PeerId, Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let dh_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&id_keys)
        .unwrap();
    let noise = NoiseConfig::xx(dh_keys);
    let info = noise.protocol_info().next().unwrap();
    let (peer_id, mut noise_io) = noise
        .upgrade_inbound(poll_data_channel, info)
        .and_then(|(remote, io)| match remote {
            RemoteIdentity::IdentityKey(pk) => future::ok((pk.to_peer_id(), io)),
            _ => future::err(NoiseError::AuthenticationFailed),
        })
        .await?;

    // Exchange TLS certificate fingerprints to prevent MiM attacks.
    debug!(
        "exchanging TLS certificate fingerprints with peer_id={}",
        peer_id
    );

    // 1. Submit SHA-256 fingerprint
    let n = noise_io
        .write(&our_fingerprint.to_owned().into_bytes())
        .await?;
    noise_io.flush().await?;

    // 2. Receive one too and compare it to the fingerprint of the remote DTLS certificate.
    let mut buf = vec![0; n]; // ASSERT: fingerprint's format is the same.
    noise_io.read_exact(buf.as_mut_slice()).await?;
    let fingerprint_from_noise =
        String::from_utf8(buf).map_err(|_| Error::Noise(NoiseError::AuthenticationFailed))?;
    if fingerprint_from_noise != remote_fingerprint {
        return Err(Error::InvalidFingerprint {
            expected: remote_fingerprint.to_owned(),
            got: fingerprint_from_noise,
        });
    }

    Ok(peer_id)
}
