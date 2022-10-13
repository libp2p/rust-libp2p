use libp2p_core::multiaddr::{Multiaddr,Protocol};
use itertools::Itertools;//TODO: replace with std intersperse when that's stable

//TODO: remove most/all of this file and replace with calls to Multiaddr::protocol_stack
//  once that lands : https://github.com/multiformats/rust-multiaddr/pull/60

pub(crate) fn protocol_stack(ma: &Multiaddr) -> String {
    ma.iter().map(tag).intersperse("/").collect()
}

pub fn tag(proto: Protocol) -> &'static str {
    use Protocol::*;
    match proto {
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
        Ws(ref s) if s == "/" => "ws",
        Ws(_) => "x-parity-ws",
        Wss(ref s) if s == "/" => "wss",
        Wss(_) => "x-parity-wss",
    }
}
