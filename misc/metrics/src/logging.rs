use libp2p::multiaddr::Protocol;
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::{Multiaddr, PeerId};
use prometheus_client::metrics::gauge::Gauge;

struct ProtocolStack(String);

impl From<Multiaddr> for ProtocolStack {
    fn from(address: Multiaddr) -> Self {
        Self(
            address
                .into_iter()
                .map(|p| match p {
                    Protocol::Dccp(_) => "dccp",
                    Protocol::Dns(_) => "dns",
                    Protocol::Dns4(_) => "dns4",
                    Protocol::Dns6(_) => "dns6",
                    Protocol::Dnsaddr(_) => "dnsaddr",
                    Protocol::Http => "http",
                    Protocol::Https => "https",
                    Protocol::Ip4(_) => "ip4",
                    Protocol::Ip6(_) => "ip6",
                    Protocol::P2pWebRtcDirect => "p2pwebrtcdirect",
                    Protocol::P2pWebRtcStar => "p2pwebrtcstar",
                    Protocol::P2pWebSocketStar => "p2pwebsocketstar",
                    Protocol::Memory(_) => "memory",
                    Protocol::Onion(_, _) => "onion",
                    Protocol::Onion3(_) => "onion3",
                    Protocol::P2p(_) => "p2p",
                    Protocol::P2pCircuit => "p2pcircuit",
                    Protocol::Quic => "quic",
                    Protocol::Sctp(_) => "sctp",
                    Protocol::Tcp(_) => "tcp",
                    Protocol::Tls => "tls",
                    Protocol::Udp(_) => "udp",
                    Protocol::Udt => "udt",
                    Protocol::Unix(_) => "unix",
                    Protocol::Utp => "utp",
                    Protocol::Ws(_) => "ws",
                    Protocol::Wss(_) => "wss",
                })
                .intersperse("/")
                .collect(),
        )
    }
}
