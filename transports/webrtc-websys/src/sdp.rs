use std::net::SocketAddr;

use libp2p_webrtc_utils::Fingerprint;
use web_sys::{RtcSdpType, RtcSessionDescriptionInit};

/// Creates the SDP answer used by the client.
pub(crate) fn answer(
    addr: SocketAddr,
    server_fingerprint: Fingerprint,
    client_ufrag: &str,
) -> RtcSessionDescriptionInit {
    let answer_obj = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
    answer_obj.set_sdp(&libp2p_webrtc_utils::sdp::answer(
        addr,
        server_fingerprint,
        client_ufrag,
    ));
    answer_obj
}

/// Creates the munged SDP offer from the Browser's given SDP offer
///
/// Certificate verification is disabled which is why we hardcode a dummy fingerprint here.
pub(crate) fn offer(offer: String, client_ufrag: &str) -> RtcSessionDescriptionInit {
    // find line and replace a=ice-ufrag: with "\r\na=ice-ufrag:{client_ufrag}\r\n"
    // find line and replace a=ice-pwd: with "\r\na=ice-ufrag:{client_ufrag}\r\n"

    let mut munged_sdp_offer = String::new();

    for line in offer.split("\r\n") {
        if line.starts_with("a=ice-ufrag:") {
            munged_sdp_offer.push_str(&format!("a=ice-ufrag:{client_ufrag}\r\n"));
            continue;
        }

        if line.starts_with("a=ice-pwd:") {
            munged_sdp_offer.push_str(&format!("a=ice-pwd:{client_ufrag}\r\n"));
            continue;
        }

        if !line.is_empty() {
            munged_sdp_offer.push_str(&format!("{}\r\n", line));
            continue;
        }
    }

    // remove any double \r\n
    let munged_sdp_offer = munged_sdp_offer.replace("\r\n\r\n", "\r\n");

    tracing::trace!(offer=%munged_sdp_offer, "Created SDP offer");

    let offer_obj = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
    offer_obj.set_sdp(&munged_sdp_offer);

    offer_obj
}
