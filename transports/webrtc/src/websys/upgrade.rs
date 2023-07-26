pub(crate) mod noise;

pub(crate) use super::fingerprint::Fingerprint;
use super::sdp;
use super::stream::{DataChannelConfig, WebRTCStream};

use super::Error;
use libp2p_identity::{Keypair, PeerId};
use web_sys::{RtcDataChannel, RtcPeerConnection};

/// Creates a new outbound WebRTC connection.
pub(crate) async fn outbound(
    peer_connection: &RtcPeerConnection,
    id_keys: Keypair,
    remote_fingerprint: Fingerprint,
) -> Result<PeerId, Error> {
    let handshake_data_channel: RtcDataChannel = DataChannelConfig::new()
        .negotiated(true)
        .id(0)
        .open(peer_connection);

    let webrtc_stream = WebRTCStream::new(handshake_data_channel);

    // get local_fingerprint from local RtcPeerConnection peer_connection certificate
    let local_sdp = match &peer_connection.local_description() {
        Some(description) => description.sdp(),
        None => return Err(Error::JsError("local_description is None".to_string())),
    };
    let local_fingerprint = match sdp::fingerprint(&local_sdp) {
        Ok(fingerprint) => fingerprint,
        Err(e) => return Err(Error::JsError(format!("fingerprint: {}", e))),
    };

    let peer_id = noise::outbound(
        id_keys,
        webrtc_stream,
        remote_fingerprint,
        local_fingerprint,
    )
    .await?;

    Ok(peer_id)
}
