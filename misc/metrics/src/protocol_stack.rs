use libp2p_core::multiaddr::Multiaddr;

pub(crate) fn as_string(ma: &Multiaddr) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip6_tcp_wss_p2p() {
        let ma = Multiaddr::try_from("/ip6/2001:8a0:7ac5:4201:3ac9:86ff:fe31:7095/tcp/8000/wss/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC").expect("testbad");

        let protocol_stack = as_string(&ma);

        assert_eq!(protocol_stack, "/ip6/tcp/wss/p2p");

        let ma = Multiaddr::try_from("/ip6/2001:8a0:7ac5:4201:3ac9:86ff:fe31:7095/tcp/8000/tls/ws/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC").expect("testbad");

        let protocol_stack = as_string(&ma);

        assert_eq!(protocol_stack, "/ip6/tcp/tls/ws/p2p");
    }
}
