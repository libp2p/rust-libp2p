use crate::tokio::fingerprint::Fingerprint;
use libp2p_webrtc_utils::sdp::render_description;
use std::net::SocketAddr;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

/// Creates the SDP answer used by the client.
pub fn answer(
    addr: SocketAddr,
    server_fingerprint: &Fingerprint,
    client_ufrag: &str,
) -> RTCSessionDescription {
    RTCSessionDescription::answer(render_description(
        SERVER_SESSION_DESCRIPTION,
        addr,
        server_fingerprint,
        client_ufrag,
    ))
    .unwrap()
}

/// Creates the SDP offer used by the server.
///
/// Certificate verification is disabled which is why we hardcode a dummy fingerprint here.
pub fn offer(addr: SocketAddr, client_ufrag: &str) -> RTCSessionDescription {
    RTCSessionDescription::offer(render_description(
        CLIENT_SESSION_DESCRIPTION,
        addr,
        &Fingerprint::from([0xFF; 32]),
        client_ufrag,
    ))
    .unwrap()
}

const CLIENT_SESSION_DESCRIPTION: &str = "v=0
o=- 0 0 IN {ip_version} {target_ip}
s=-
c=IN {ip_version} {target_ip}
t=0 0

m=application {target_port} UDP/DTLS/SCTP webrtc-datachannel
a=mid:0
a=ice-options:ice2
a=ice-ufrag:{ufrag}
a=ice-pwd:{pwd}
a=fingerprint:{fingerprint_algorithm} {fingerprint_value}
a=setup:actpass
a=sctp-port:5000
a=max-message-size:16384
";

const SERVER_SESSION_DESCRIPTION: &str = "v=0
o=- 0 0 IN {ip_version} {target_ip}
s=-
t=0 0
a=ice-lite
m=application {target_port} UDP/DTLS/SCTP webrtc-datachannel
c=IN {ip_version} {target_ip}
a=mid:0
a=ice-options:ice2
a=ice-ufrag:{ufrag}
a=ice-pwd:{pwd}
a=fingerprint:{fingerprint_algorithm} {fingerprint_value}

a=setup:passive
a=sctp-port:5000
a=max-message-size:16384
a=candidate:1 1 UDP 1 {target_ip} {target_port} typ host
a=end-of-candidates
";
