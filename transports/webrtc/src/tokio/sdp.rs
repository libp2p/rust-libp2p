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

use serde::Serialize;
use tinytemplate::TinyTemplate;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

use std::net::{IpAddr, SocketAddr};

use crate::tokio::fingerprint::Fingerprint;

/// Creates the SDP answer used by the client.
pub(crate) fn answer(
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
pub(crate) fn offer(addr: SocketAddr, client_ufrag: &str) -> RTCSessionDescription {
    RTCSessionDescription::offer(render_description(
        CLIENT_SESSION_DESCRIPTION,
        addr,
        &Fingerprint::FF,
        client_ufrag,
    ))
    .unwrap()
}

// An SDP message that constitutes the offer.
//
// Main RFC: <https://datatracker.ietf.org/doc/html/rfc8866>
// `sctp-port` and `max-message-size` attrs RFC: <https://datatracker.ietf.org/doc/html/rfc8841>
// `group` and `mid` attrs RFC: <https://datatracker.ietf.org/doc/html/rfc9143>
// `ice-ufrag`, `ice-pwd` and `ice-options` attrs RFC: <https://datatracker.ietf.org/doc/html/rfc8839>
// `setup` attr RFC: <https://datatracker.ietf.org/doc/html/rfc8122>
//
// Short description:
//
// v=<protocol-version> -> always 0
// o=<username> <sess-id> <sess-version> <nettype> <addrtype> <unicast-address>
//
//     <username> identifies the creator of the SDP document. We are allowed to use dummy values
//     (`-` and `0.0.0.0` as <addrtype>) to remain anonymous, which we do. Note that "IN" means
//     "Internet".
//
// s=<session name>
//
//     We are allowed to pass a dummy `-`.
//
// c=<nettype> <addrtype> <connection-address>
//
//     Indicates the IP address of the remote.
//     Note that "IN" means "Internet".
//
// t=<start-time> <stop-time>
//
//     Start and end of the validity of the session. `0 0` means that the session never expires.
//
// m=<media> <port> <proto> <fmt> ...
//
//     A `m=` line describes a request to establish a certain protocol. The protocol in this line
//     (i.e. `TCP/DTLS/SCTP` or `UDP/DTLS/SCTP`) must always be the same as the one in the offer.
//     We know that this is true because we tweak the offer to match the protocol. The `<fmt>`
//     component must always be `webrtc-datachannel` for WebRTC.
//     RFCs: 8839, 8866, 8841
//
// a=mid:<MID>
//
//     Media ID - uniquely identifies this media stream (RFC9143).
//
// a=ice-options:ice2
//
//     Indicates that we are complying with RFC8839 (as oppposed to the legacy RFC5245).
//
// a=ice-ufrag:<ICE user>
// a=ice-pwd:<ICE password>
//
//     ICE username and password, which are used for establishing and
//     maintaining the ICE connection. (RFC8839)
//     MUST match ones used by the answerer (server).
//
// a=fingerprint:sha-256 <fingerprint>
//
//     Fingerprint of the certificate that the remote will use during the TLS
//     handshake. (RFC8122)
//
// a=setup:actpass
//
//     The endpoint that is the offerer MUST use the setup attribute value of setup:actpass and be
//     prepared to receive a client_hello before it receives the answer.
//
// a=sctp-port:<value>
//
//     The SCTP port (RFC8841)
//     Note it's different from the "m=" line port value, which indicates the port of the
//     underlying transport-layer protocol (UDP or TCP).
//
// a=max-message-size:<value>
//
//     The maximum SCTP user message size (in bytes). (RFC8841)
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
// a=candidate:<foundation> <component-id> <transport> <priority> <connection-address> <port> <cand-type>
//
//     A transport address for a candidate that can be used for connectivity checks (RFC8839).
//
// a=end-of-candidates
//
//     Indicate that no more candidates will ever be sent (RFC8838).
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
fn render_description(
    description: &str,
    addr: SocketAddr,
    fingerprint: &Fingerprint,
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
