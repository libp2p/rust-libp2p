use libp2p_core::multiaddr::{Multiaddr, Protocol};
use prometheus_client::encoding::text::Encode;

#[derive(Encode, Hash, Clone, Eq, PartialEq)]
pub struct Labels {
    protocols: String,
}

impl Labels {
    pub fn new(ma: &Multiaddr) -> Self {
        Self {
            protocols: ma.protocol_stack(),
        }
    }
}

//TODO: remove this trait and tag() and replace with calls to the upstream method
//  once that lands : https://github.com/multiformats/rust-multiaddr/pull/60
// In the meantime there is no _ case in the match so one can easily detect mismatch in supported
//  protocols when dependency version changes.
pub trait MultiaddrExt {
    fn protocol_stack(&self) -> String;
}

impl MultiaddrExt for Multiaddr {
    fn protocol_stack(&self) -> String {
        // Has potential to allocate multiple times, but this line expresses the intent here.
        //  std::iter::once("/").chain(ma.iter().map(tag).intersperse("/")).collect()
        let len = self.iter().fold(0, |acc, proto| acc + tag(proto).len() + 1);
        let mut result = String::with_capacity(len);
        for proto_tag in self.iter().map(tag) {
            result.push('/');
            result.push_str(proto_tag);
        }
        result
    }
}

fn tag(proto: Protocol) -> &'static str {
    use Protocol::*;
    match proto {
        Certhash(_) => "certhash",
        Dccp(_) => "dccp",
        Dns(_) => "dns",
        Dns4(_) => "dns4",
        Dns6(_) => "dns6",
        Dnsaddr(_) => "dnsaddr",
        Http => "http",
        Https => "https",
        Ip4(_) => "ip4",
        Ip6(_) => "ip6",
        P2pWebRtcDirect => "p2p-webrtc-direct",
        P2pWebRtcStar => "p2p-webrtc-star",
        P2pWebSocketStar => "p2p-websocket-star",
        Memory(_) => "memory",
        Noise => "noise",
        Onion(_, _) => "onion",
        Onion3(_) => "onion3",
        P2p(_) => "p2p",
        P2pCircuit => "p2p-circuit",
        Quic => "quic",
        Sctp(_) => "sctp",
        Tcp(_) => "tcp",
        Tls => "tls",
        Udp(_) => "udp",
        Udt => "udt",
        Unix(_) => "unix",
        Utp => "utp",
        WebRTC => "webrtc",
        Ws(ref s) if s == "/" => "ws",
        Ws(_) => "x-parity-ws",
        Wss(ref s) if s == "/" => "wss",
        Wss(_) => "x-parity-wss",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip6_tcp_wss_p2p() {
        let ma = Multiaddr::try_from("/ip6/2001:8a0:7ac5:4201:3ac9:86ff:fe31:7095/tcp/8000/wss/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC").expect("testbad");
        let actual = Labels::for_multi_address(&ma);
        assert_eq!(actual.address_stack, "/ip6/tcp/wss/p2p");
        let mut buf = Vec::new();
        actual.encode(&mut buf).expect("encode failed");
        let actual = String::from_utf8(buf).expect("invalid utf-8?");
        assert_eq!(actual, r#"address_stack="/ip6/tcp/wss/p2p""#);
    }
}
