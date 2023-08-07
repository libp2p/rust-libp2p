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

use super::fingerprint::Fingerprint;
use js_sys::Reflect;
use serde::Serialize;
use std::net::{IpAddr, SocketAddr};
use tinytemplate::TinyTemplate;
use wasm_bindgen::JsValue;
use web_sys::{RtcSdpType, RtcSessionDescriptionInit};

/// Creates the SDP answer used by the client.
pub(crate) fn answer(
    addr: SocketAddr,
    server_fingerprint: &Fingerprint,
    client_ufrag: &str,
) -> RtcSessionDescriptionInit {
    let mut answer_obj = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
    answer_obj.sdp(&render_description(
        SESSION_DESCRIPTION,
        addr,
        server_fingerprint,
        client_ufrag,
    ));
    answer_obj
}

/// Creates the SDP offer.
///
/// Certificate verification is disabled which is why we hardcode a dummy fingerprint here.
pub(crate) fn offer(offer: JsValue, client_ufrag: &str) -> RtcSessionDescriptionInit {
    //JsValue to String
    let offer = Reflect::get(&offer, &JsValue::from_str("sdp")).unwrap();
    let offer = offer.as_string().unwrap();

    let lines = offer.split("\r\n");

    // find line and replace a=ice-ufrag: with "\r\na=ice-ufrag:{client_ufrag}\r\n"
    // find line andreplace a=ice-pwd: with "\r\na=ice-ufrag:{client_ufrag}\r\n"

    let mut munged_offer_sdp = String::new();

    for line in lines {
        if line.starts_with("a=ice-ufrag:") {
            munged_offer_sdp.push_str(&format!("a=ice-ufrag:{}\r\n", client_ufrag));
        } else if line.starts_with("a=ice-pwd:") {
            munged_offer_sdp.push_str(&format!("a=ice-pwd:{}\r\n", client_ufrag));
        } else if !line.is_empty() {
            munged_offer_sdp.push_str(&format!("{}\r\n", line));
        }
    }

    // remove any double \r\n
    let munged_offer_sdp = munged_offer_sdp.replace("\r\n\r\n", "\r\n");

    log::trace!("munged_offer_sdp: {}", munged_offer_sdp);

    // setLocalDescription
    let mut offer_obj = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
    offer_obj.sdp(&munged_offer_sdp);

    offer_obj
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
// a=candidate:<foundation> <component-id> <transport> <priority> <connection-address> <port> <cand-type>
//
//     A transport address for a candidate that can be used for connectivity checks (RFC8839).
//
// a=end-of-candidates
//
//     Indicate that no more candidates will ever be sent (RFC8838).
// const SERVER_SESSION_DESCRIPTION: &str = "v=0
// o=- 0 0 IN {ip_version} {target_ip}
// s=-
// t=0 0
// a=ice-lite
// m=application {target_port} UDP/DTLS/SCTP webrtc-datachannel
// c=IN {ip_version} {target_ip}
// a=mid:0
// a=ice-options:ice2
// a=ice-ufrag:{ufrag}
// a=ice-pwd:{pwd}
// a=fingerprint:{fingerprint_algorithm} {fingerprint_value}

// a=setup:passive
// a=sctp-port:5000
// a=max-message-size:16384
// a=candidate:1 1 UDP 1 {target_ip} {target_port} typ host
// a=end-of-candidates";

// Update to this:
// v=0
// o=- 0 0 IN ${ipVersion} ${host}
// s=-
// c=IN ${ipVersion} ${host}
// t=0 0
// a=ice-lite
// m=application ${port} UDP/DTLS/SCTP webrtc-datachannel
// a=mid:0
// a=setup:passive
// a=ice-ufrag:${ufrag}
// a=ice-pwd:${ufrag}
// a=fingerprint:${CERTFP}
// a=sctp-port:5000
// a=max-message-size:100000
// a=candidate:1467250027 1 UDP 1467250027 ${host} ${port} typ host\r\n
const SESSION_DESCRIPTION: &str = "v=0
o=- 0 0 IN {ip_version} {target_ip}
s=-
c=IN {ip_version} {target_ip}
t=0 0
a=ice-lite
m=application {target_port} UDP/DTLS/SCTP webrtc-datachannel
a=mid:0
a=setup:passive
a=ice-ufrag:{ufrag}
a=ice-pwd:{pwd}
a=fingerprint:{fingerprint_algorithm} {fingerprint_value}
a=sctp-port:5000
a=max-message-size:16384
a=candidate:1467250027 1 UDP 1467250027 {target_ip} {target_port} typ host
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

/// Get Fingerprint from SDP
/// Gets the fingerprint from matching between the angle brackets: a=fingerprint:<hash-algo> <fingerprint>
pub fn fingerprint(sdp: &str) -> Option<Fingerprint> {
    // split the sdp by new lines / carriage returns
    let lines = sdp.split("\r\n");

    // iterate through the lines to find the one starting with a=fingerprint:
    // get the value after the first space
    // return the value as a Fingerprint
    for line in lines {
        if line.starts_with("a=fingerprint:") {
            let fingerprint = line.split(' ').nth(1).unwrap();
            let bytes = hex::decode(fingerprint.replace(':', "")).unwrap();
            let arr: [u8; 32] = bytes.as_slice().try_into().unwrap();
            return Some(Fingerprint::raw(arr));
        }
    }
    None
}

#[cfg(test)]
mod sdp_tests {
    use super::*;

    #[test]
    fn test_fingerprint() {
        let sdp: &str = "v=0\no=- 0 0 IN IP6 ::1\ns=-\nc=IN IP6 ::1\nt=0 0\na=ice-lite\nm=application 61885 UDP/DTLS/SCTP webrtc-datachannel\na=mid:0\na=setup:passive\na=ice-ufrag:libp2p+webrtc+v1/YwapWySn6fE6L9i47PhlB6X4gzNXcgFs\na=ice-pwd:libp2p+webrtc+v1/YwapWySn6fE6L9i47PhlB6X4gzNXcgFs\na=fingerprint:sha-256 A8:17:77:1E:02:7E:D1:2B:53:92:70:A6:8E:F9:02:CC:21:72:3A:92:5D:F4:97:5F:27:C4:5E:75:D4:F4:31:89\na=sctp-port:5000\na=max-message-size:16384\na=candidate:1467250027 1 UDP 1467250027 ::1 61885 typ host\n";
        let fingerprint = match fingerprint(sdp) {
            Some(fingerprint) => fingerprint,
            None => panic!("No fingerprint found"),
        };
        assert_eq!(fingerprint.algorithm(), "sha-256");
        assert_eq!(fingerprint.to_sdp_format(), "A8:17:77:1E:02:7E:D1:2B:53:92:70:A6:8E:F9:02:CC:21:72:3A:92:5D:F4:97:5F:27:C4:5E:75:D4:F4:31:89");
    }
}
