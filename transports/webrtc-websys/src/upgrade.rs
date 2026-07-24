use std::net::SocketAddr;

use libp2p_identity::{Keypair, PeerId};
use libp2p_webrtc_utils::{Fingerprint, StreamConfig, noise};
use send_wrapper::SendWrapper;

use super::Error;
use crate::{Connection, connection::RtcPeerConnection, error::AuthenticationError, sdp};

/// Upgrades an outbound WebRTC connection by creating the data channel
/// and conducting a Noise handshake
pub(crate) async fn outbound(
    sock_addr: SocketAddr,
    remote_fingerprint: Fingerprint,
    id_keys: Keypair,
    stream_config: StreamConfig,
) -> Result<(PeerId, Connection), Error> {
    let fut = SendWrapper::new(outbound_inner(
        sock_addr,
        remote_fingerprint,
        id_keys,
        stream_config,
    ));
    fut.await
}

/// Inner outbound function that is wrapped in [SendWrapper]
async fn outbound_inner(
    sock_addr: SocketAddr,
    remote_fingerprint: Fingerprint,
    id_keys: Keypair,
    stream_config: StreamConfig,
) -> Result<(PeerId, Connection), Error> {
    let rtc_peer_connection = RtcPeerConnection::new(remote_fingerprint.algorithm()).await?;

    // Create stream for Noise handshake
    // Must create data channel before Offer is created for it to be included in the SDP
    let (channel, listener) = rtc_peer_connection.new_handshake_stream(stream_config);
    drop(listener);

    let ufrag = libp2p_webrtc_utils::sdp::random_ufrag();

    let offer = rtc_peer_connection.create_offer().await?;
    let munged_offer = sdp::offer(offer, &ufrag);
    rtc_peer_connection
        .set_local_description(munged_offer)
        .await?;

    let answer = sdp::answer(sock_addr, remote_fingerprint, &ufrag);
    rtc_peer_connection.set_remote_description(answer).await?;

    let local_fingerprint = rtc_peer_connection.local_fingerprint()?;

    tracing::trace!(?local_fingerprint);
    tracing::trace!(?remote_fingerprint);

    let (peer_id, stream_config) = noise::outbound_with_message_size(
        id_keys,
        channel,
        remote_fingerprint,
        local_fingerprint,
        stream_config,
    )
    .await
    .map_err(AuthenticationError)?;

    tracing::debug!(peer=%peer_id, "Remote peer identified");

    Ok((peer_id, Connection::new(rtc_peer_connection, stream_config)))
}
