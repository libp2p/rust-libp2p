//! The listener presence module allows for easy and fast checking if there is a listener present
//! for a multiaddress we want to dial.

use std::collections::HashMap;
use libp2p_core::Multiaddr;

type ProtocolStack = Vec<&'static str>;

// I'm not sure about the selection here. A good example is Onion. It's not good defined what that's supposed to be and it's a protocol and an address. But since I wrote the only Tor transport for libp2p and it doesn't support these Onion addresses, I will just ignore that.
// We might also choose to ignore encryption, like noise and tls
const NON_PROTOCOL_TAGS: &[&str] = &["dns", "dns4", "dns6", "dnsaddr", "ip4", "ip6", "p2p"];

fn is_not_protocol(tag: &str) -> bool {
    // using contains instead of matches! isn't a lot different, when we look at the generated assembly (https://godbolt.org/z/1x9f3K16x)
    !NON_PROTOCOL_TAGS.contains(&tag)
}

// Turns a multiaddress into a vector of just the protocols.
fn clean_multiaddr(address: &Multiaddr) -> ProtocolStack {
    address
        .protocol_stack()
        .filter(|e| is_not_protocol(e))
        .collect()
}

#[derive(Debug, Clone, Default)]
pub(super) struct ListenerPresence {
    inner: HashMap<ProtocolStack, u8>,
}

#[cfg(test)]
impl<'a> FromIterator<&'a Multiaddr> for ListenerPresence {
    fn from_iter<T: IntoIterator<Item = &'a Multiaddr>>(iter: T) -> Self {
        let inner = iter
            .into_iter()
            .map(clean_multiaddr)
            .map(|key| (key, 1))
            .collect();
        Self { inner }
    }
}

impl ListenerPresence {
    pub fn contains(&self, address: &Multiaddr) -> bool {
        let protocol_stack = clean_multiaddr(address);
        #[cfg(debug_assertions)]
        if let Some(key) = self.inner.get(&protocol_stack) {
            debug_assert_ne!(*key, 0, "The entry value should never be zero and exist.");
        }
        self.inner.contains_key(&protocol_stack)
    }

    pub fn new_listener(&mut self, address: &Multiaddr) {
        self.inner
            .entry(clean_multiaddr(address))
            .and_modify(|e| *e += 1)
            .or_insert(1);
    }

    pub fn expired_listener(&mut self, address: &Multiaddr) {
        let protocol_stack = clean_multiaddr(address);
        match self.inner.get_mut(&protocol_stack) {
            Some(1) => {
                self.inner.remove(&protocol_stack);
            }
            Some(n) => {
                *n -= 1;
            }
            None => {}
        }
    }
}

// Some tests with real world data would be appreciated
#[cfg(test)]
mod tests {
    use crate::listener_presence::{clean_multiaddr, ListenerPresence, NON_PROTOCOL_TAGS};
    use libp2p_core::multiaddr::{multiaddr, Protocol};
    use libp2p_identity::PeerId;
    use std::borrow::Cow;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use std::str::FromStr;

    #[test]
    fn basic_ops() {
        let bootstrap_libp2p_node_peer_id =
            PeerId::from_str("QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN").unwrap();
        let test_addrs = [
            multiaddr!(Ip4([127, 0, 0, 1]), Tcp(1234u16)),
            multiaddr!(
                Ip6([11, 22, 33, 44, 55, 66, 77, 88]),
                Udp(199u16),
                Tls,
                Quic
            ),
            multiaddr!(Dns4("heise.de"), Tcp(443u16), Tls, Https),
            multiaddr!(
                Dnsaddr("bootstrap.libp2p.io"),
                P2p(bootstrap_libp2p_node_peer_id)
            ),
            multiaddr!(
                Ip4([104, 131, 131, 82]),
                Udp(4001u16),
                Quic,
                P2p(bootstrap_libp2p_node_peer_id)
            ),
        ];
        let listener_presence: ListenerPresence = test_addrs.iter().collect();
        assert!(
            test_addrs
                .iter()
                .all(|addr| listener_presence.contains(addr)),
            "Basic input operations are not working. Likely cleaning function is not pure."
        );
    }

    #[test]
    fn reducing_functionality() {
        let build_up_address = [
            multiaddr!(Dnsaddr("libp2p.io"), Tls, Tcp(10u16)),
            multiaddr!(Dnsaddr("libp2p.io"), Tls, Tcp(12u16), Udp(13u16), Quic),
            multiaddr!(Ip4([1, 1, 1, 1]), Udp(100u16)),
        ];
        let listener_presence: ListenerPresence = build_up_address.iter().collect();
        assert!(build_up_address
            .iter()
            .all(|addr| listener_presence.contains(addr)));
        assert_eq!(clean_multiaddr(&build_up_address[0]), clean_multiaddr(&multiaddr!(Dns4("libp2p.io"), Tls, Tcp(10u16))));
        assert!(listener_presence.contains(&multiaddr!(Dns4("libp2p.io"), Tls, Tcp(10u16))));
        assert!(listener_presence.contains(&multiaddr!(
            Dns4("libp2p.io"),
            Tls,
            Tcp(10u16),
            Dnsaddr("bootstrap.libp2p.io")
        )));
        assert!(listener_presence.contains(&multiaddr!(Dns("one.one.one.one"), Tls, Tcp(100u16))));
        assert!(!listener_presence.contains(&multiaddr!(Dns("one.one.one.one"), Tcp(100u16))));
        assert!(!listener_presence.contains(&multiaddr!(Dnsaddr("libp2p.io"), Tcp(10u16), Tls)));
        assert!(!listener_presence.contains(&multiaddr!(
            Dnsaddr("libp2p.io"),
            Quic,
            Udp(13u16),
            Tcp(12u16),
            Tls
        )));
        assert!(!listener_presence.contains(&multiaddr!(
            Dnsaddr("one.one.one.one"),
            Udp(100u16),
            Tls
        )));
    }

    #[test]
    fn tags_correct() {
        let protocols = &[
            Protocol::Dns(Cow::Borrowed("")),
            Protocol::Dns4(Cow::Borrowed("")),
            Protocol::Dns6(Cow::Borrowed("")),
            Protocol::Dnsaddr(Cow::Borrowed("")),
            Protocol::Ip4(Ipv4Addr::new(1, 1, 1, 1)),
            Protocol::Ip6(Ipv6Addr::new(1, 1, 1, 1, 1, 1, 1, 1)),
            Protocol::P2p(PeerId::random()),
        ];
        assert_eq!(protocols.len(), NON_PROTOCOL_TAGS.len());
        protocols
            .iter()
            .map(|e| e.tag())
            .zip(NON_PROTOCOL_TAGS.iter())
            .for_each(|(a, b)| assert_eq!(a, *b));
    }
}
