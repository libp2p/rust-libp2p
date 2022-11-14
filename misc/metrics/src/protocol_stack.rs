use libp2p_core::multiaddr::Multiaddr;
use prometheus_client::encoding::text::Encode;

pub fn as_string(ma: &Multiaddr) -> String {
    let len = ma
        .protocol_stack()
        .fold(0, |acc, proto| acc + proto.len() + 1);
    let mut protocols = String::with_capacity(len);
    for proto_tag in ma.protocol_stack() {
        protocols.push('/');
        protocols.push_str(proto_tag);
    }
    protocols
}

#[derive(Encode, Hash, Clone, Eq, PartialEq)]
pub struct Labels {
    protocols: String,
}

impl Labels {
    pub fn new(ma: &Multiaddr) -> Self {
        Self {
            protocols: as_string(ma),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip6_tcp_wss_p2p() {
        let ma = Multiaddr::try_from("/ip6/2001:8a0:7ac5:4201:3ac9:86ff:fe31:7095/tcp/8000/wss/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC").expect("testbad");
        let actual = Labels::new(&ma);
        assert_eq!(actual.protocols, "/ip6/tcp/wss/p2p");
        let mut buf = Vec::new();
        actual.encode(&mut buf).expect("encode failed");
        let actual = String::from_utf8(buf).expect("invalid utf-8?");
        assert_eq!(actual, r#"protocols="/ip6/tcp/wss/p2p""#);
    }
}
