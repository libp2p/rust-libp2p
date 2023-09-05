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

use crate::tokio::fingerprint::Fingerprint;
pub(crate) use libp2p_webrtc_utils::sdp::random_ufrag;
use libp2p_webrtc_utils::sdp::render_description;
use std::net::SocketAddr;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

/// Creates the SDP answer used by the client.
pub(crate) fn answer(
    addr: SocketAddr,
    server_fingerprint: &Fingerprint,
    client_ufrag: &str,
) -> RTCSessionDescription {
    RTCSessionDescription::answer(libp2p_webrtc_utils::sdp::answer(
        addr,
        server_fingerprint,
        client_ufrag,
    ))
    .unwrap()
}

/// Creates the SDP offer used by the server.
///
/// Certificate verification is disabled which is why we hardcode a dummy fingerprint here.
pub(crate) fn offer(addr: SocketAddr, client_ufrag: &str) -> RTCSessionDescription {
    let offer = render_description(
        CLIENT_SESSION_DESCRIPTION,
        addr,
        &Fingerprint::from([0xFF; 32]),
        client_ufrag,
    );

    log::trace!("Created SDP offer: {offer}");

    RTCSessionDescription::offer(offer).unwrap()
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
