pub mod noise;

use crate::fingerprint::Fingerprint;
use crate::sdp;
use crate::stream::{DataChannelConfig, WebRTCStream};

use crate::Error;
use libp2p_identity::{Keypair, PeerId};
use web_sys::{RtcDataChannel, RtcDataChannelInit, RtcPeerConnection};

/// Creates a new outbound WebRTC connection.
pub(crate) async fn outbound(
    peer_connection: &RtcPeerConnection,
    id_keys: Keypair,
    remote_fingerprint: Fingerprint,
) -> Result<PeerId, Error> {
    let data_channel = create_substream_for_noise_handshake(&peer_connection).await?;

    // get local_fingerprint from local RtcPeerConnection peer_connection certificate
    let local_sdp = match &peer_connection.local_description() {
        Some(description) => description.sdp(),
        None => return Err(Error::JsError("local_description is None".to_string())),
    };
    let local_fingerprint = match sdp::fingerprint(&local_sdp) {
        Ok(fingerprint) => fingerprint,
        Err(e) => return Err(Error::JsError(format!("fingerprint: {}", e))),
    };

    let peer_id =
        noise::outbound(id_keys, data_channel, remote_fingerprint, local_fingerprint).await?;

    Ok(peer_id)
}

pub async fn create_substream_for_noise_handshake(
    conn: &RtcPeerConnection,
) -> Result<WebRTCStream, Error> {
    // NOTE: the data channel w/ `negotiated` flag set to `true` MUST be created on both ends.
    let handshake_data_channel: RtcDataChannel =
        DataChannelConfig::new().negotiated(true).id(0).open(&conn);

    Ok(WebRTCStream::new(handshake_data_channel))
}
