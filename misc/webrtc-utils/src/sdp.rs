// Copyright 2023 Doug Anderson
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
use std::net::{IpAddr, SocketAddr};

use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde::Serialize;
use tinytemplate::TinyTemplate;

use crate::fingerprint::Fingerprint;

pub fn answer(addr: SocketAddr, server_fingerprint: Fingerprint, client_ufrag: &str) -> String {
    let answer = render_description(
        SERVER_SESSION_DESCRIPTION,
        addr,
        server_fingerprint,
        client_ufrag,
    );

    tracing::trace!(%answer, "Created SDP answer");

    answer
}

// See [`CLIENT_SESSION_DESCRIPTION`].
//
// a=ice-lite
//
//     A lite implementation is only appropriate for devices that will *always* be connected to
//     the public Internet and have a public IP address at which it can receive packets from any
//     correspondent. ICE will not function when a lite implementation is placed behind a NAT
//     (RFC8445).
//
// a=tls-id:<id>
//
//     "TLS ID" uniquely identifies a TLS association.
//     The ICE protocol uses a "TLS ID" system to indicate whether a fresh DTLS connection
//     must be reopened in case of ICE renegotiation. Considering that ICE renegotiations
//     never happen in our use case, we can simply put a random value and not care about
//     it. Note however that the TLS ID in the answer must be present if and only if the
//     offer contains one. (RFC8842)
//     TODO: is it true that renegotiations never happen? what about a connection closing?
//     "tls-id" attribute MUST be present in the initial offer and respective answer (RFC8839).
//     XXX: but right now browsers don't send it.
//
// a=setup:passive
//
//     "passive" indicates that the remote DTLS server will only listen for incoming
//     connections. (RFC5763)
//     The answerer (server) MUST not be located behind a NAT (RFC6135).
//
//     The answerer MUST use either a setup attribute value of setup:active or setup:passive.
//     Note that if the answerer uses setup:passive, then the DTLS handshake will not begin until
//     the answerer is received, which adds additional latency. setup:active allows the answer and
//     the DTLS handshake to occur in parallel. Thus, setup:active is RECOMMENDED.
//
// a=candidate:<foundation> <component-id> <transport> <priority> <connection-address> <port>
// <cand-type>
//
//     A transport address for a candidate that can be used for connectivity checks (RFC8839).
//
// a=end-of-candidates
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
a=candidate:1467250027 1 UDP 1467250027 {target_ip} {target_port} typ host
a=end-of-candidates
";

/// Indicates the IP version used in WebRTC: `IP4` or `IP6`.
#[derive(Serialize)]
enum IpVersion {
    IP4,
    IP6,
}

/// Context passed to the templating engine, which replaces the above placeholders (e.g.
/// `{IP_VERSION}`) with real values.
#[derive(Serialize)]
struct DescriptionContext {
    pub(crate) ip_version: IpVersion,
    pub(crate) target_ip: IpAddr,
    pub(crate) target_port: u16,
    pub(crate) fingerprint_algorithm: String,
    pub(crate) fingerprint_value: String,
    pub(crate) ufrag: String,
    pub(crate) pwd: String,
}

/// Renders a [`TinyTemplate`] description using the provided arguments.
pub fn render_description(
    description: &str,
    addr: SocketAddr,
    fingerprint: Fingerprint,
    ufrag: &str,
) -> String {
    let mut tt = TinyTemplate::new();
    tt.add_template("description", description).unwrap();

    let context = DescriptionContext {
        ip_version: {
            if addr.is_ipv4() {
                IpVersion::IP4
            } else {
                IpVersion::IP6
            }
        },
        target_ip: addr.ip(),
        target_port: addr.port(),
        fingerprint_algorithm: fingerprint.algorithm(),
        fingerprint_value: fingerprint.to_sdp_format(),
        // NOTE: ufrag is equal to pwd.
        ufrag: ufrag.to_owned(),
        pwd: ufrag.to_owned(),
    };
    tt.render("description", &context).unwrap()
}

/// Generates a random ufrag and adds a prefix according to the spec.
pub fn random_ufrag() -> String {
    format!(
        "libp2p+webrtc+v1/{}",
        thread_rng()
            .sample_iter(&Alphanumeric)
            .take(64)
            .map(char::from)
            .collect::<String>()
    )
}
