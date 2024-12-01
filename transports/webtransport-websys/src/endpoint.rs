use std::collections::HashSet;

use js_sys::{Array, Uint8Array};
use libp2p_identity::PeerId;
use multiaddr::{Multiaddr, Protocol};
use multihash::Multihash;

use crate::{
    bindings::{WebTransportHash, WebTransportOptions},
    Error,
};

pub(crate) struct Endpoint {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) is_ipv6: bool,
    pub(crate) certhashes: HashSet<Multihash<64>>,
    pub(crate) remote_peer: Option<PeerId>,
}

impl Endpoint {
    pub(crate) fn from_multiaddr(addr: &Multiaddr) -> Result<Self, Error> {
        let mut host = None;
        let mut port = None;
        let mut found_quic = false;
        let mut found_webtransport = false;
        let mut certhashes = HashSet::new();
        let mut remote_peer = None;
        let mut is_ipv6 = false;

        for proto in addr.iter() {
            match proto {
                Protocol::Ip4(addr) => {
                    if host.is_some() {
                        return Err(Error::InvalidMultiaddr("More than one host definitions"));
                    }

                    host = Some(addr.to_string());
                }
                Protocol::Ip6(addr) => {
                    if host.is_some() {
                        return Err(Error::InvalidMultiaddr("More than one host definitions"));
                    }

                    is_ipv6 = true;
                    host = Some(addr.to_string());
                }
                Protocol::Dns(domain) | Protocol::Dns4(domain) | Protocol::Dns6(domain) => {
                    if port.is_some() {
                        return Err(Error::InvalidMultiaddr("More than one host definitions"));
                    }

                    host = Some(domain.to_string())
                }
                Protocol::Dnsaddr(_) => {
                    return Err(Error::InvalidMultiaddr(
                        "/dnsaddr not supported from within a browser",
                    ));
                }
                Protocol::Udp(p) => {
                    if port.is_some() {
                        return Err(Error::InvalidMultiaddr("More than one port definitions"));
                    }

                    port = Some(p);
                }
                Protocol::Quic | Protocol::QuicV1 => {
                    if host.is_none() || port.is_none() {
                        return Err(Error::InvalidMultiaddr(
                            "No host and port definition before /quic/webtransport",
                        ));
                    }

                    found_quic = true;
                }
                Protocol::WebTransport => {
                    if !found_quic {
                        return Err(Error::InvalidMultiaddr(
                            "/quic is not found before /webtransport",
                        ));
                    }

                    found_webtransport = true;
                }
                Protocol::Certhash(hash) => {
                    if !found_webtransport {
                        return Err(Error::InvalidMultiaddr(
                            "/certhashes must be after /quic/found_webtransport",
                        ));
                    }

                    certhashes.insert(hash);
                }
                Protocol::P2p(peer) => {
                    if remote_peer.is_some() {
                        return Err(Error::InvalidMultiaddr("More than one peer definitions"));
                    }

                    remote_peer = Some(peer);
                }
                _ => {}
            }
        }

        if !found_quic || !found_webtransport {
            return Err(Error::InvalidMultiaddr(
                "Not a /quic/webtransport multiaddr",
            ));
        }

        let host = host.ok_or_else(|| Error::InvalidMultiaddr("Host is not defined"))?;
        let port = port.ok_or_else(|| Error::InvalidMultiaddr("Port is not defined"))?;

        Ok(Endpoint {
            host,
            port,
            is_ipv6,
            certhashes,
            remote_peer,
        })
    }

    pub(crate) fn url(&self) -> String {
        let host = &self.host;
        let port = self.port;

        if self.is_ipv6 {
            format!("https://[{host}]:{port}/.well-known/libp2p-webtransport?type=noise")
        } else {
            format!("https://{host}:{port}/.well-known/libp2p-webtransport?type=noise")
        }
    }

    pub(crate) fn webtransport_opts(&self) -> WebTransportOptions {
        let mut opts = WebTransportOptions::new();
        let hashes = Array::new();

        for hash in &self.certhashes {
            let digest = Uint8Array::from(hash.digest());

            let mut jshash = WebTransportHash::new();
            jshash.algorithm("sha-256").value(&digest);

            hashes.push(&jshash);
        }

        opts.server_certificate_hashes(&hashes);

        opts
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn multihash_from_str(s: &str) -> Multihash<64> {
        let (_base, bytes) = multibase::decode(s).unwrap();
        Multihash::from_bytes(&bytes).unwrap()
    }

    #[test]
    fn valid_webtransport_multiaddr() {
        let addr = Multiaddr::from_str("/ip4/127.0.0.1/udp/44874/quic-v1/webtransport/certhash/uEiCaDd1Ca1A8IVJ3hsIxIyi11cwxaDKqzVrBkGJbKZU5ng/certhash/uEiDv-VGW8oXxui_G_Kqp-87YjvET-Hr2qYAMYPePJDcsjQ/p2p/12D3KooWR7EfNv5SLtgjMRjUwR8AvNu3hP4fLrtSa9fmHHXKYWNG").unwrap();
        let endpoint = Endpoint::from_multiaddr(&addr).unwrap();

        assert_eq!(endpoint.host, "127.0.0.1");
        assert_eq!(endpoint.port, 44874);
        assert_eq!(endpoint.certhashes.len(), 2);

        assert!(endpoint.certhashes.contains(&multihash_from_str(
            "uEiCaDd1Ca1A8IVJ3hsIxIyi11cwxaDKqzVrBkGJbKZU5ng"
        )));

        assert!(endpoint.certhashes.contains(&multihash_from_str(
            "uEiDv-VGW8oXxui_G_Kqp-87YjvET-Hr2qYAMYPePJDcsjQ"
        )));

        assert_eq!(
            endpoint.remote_peer.unwrap(),
            PeerId::from_str("12D3KooWR7EfNv5SLtgjMRjUwR8AvNu3hP4fLrtSa9fmHHXKYWNG").unwrap()
        );

        assert_eq!(
            endpoint.url(),
            "https://127.0.0.1:44874/.well-known/libp2p-webtransport?type=noise"
        );
    }

    #[test]
    fn valid_webtransport_multiaddr_without_certhashes() {
        let addr = Multiaddr::from_str("/ip4/127.0.0.1/udp/44874/quic-v1/webtransport/p2p/12D3KooWR7EfNv5SLtgjMRjUwR8AvNu3hP4fLrtSa9fmHHXKYWNG").unwrap();
        let endpoint = Endpoint::from_multiaddr(&addr).unwrap();

        assert_eq!(endpoint.host, "127.0.0.1");
        assert_eq!(endpoint.port, 44874);
        assert_eq!(endpoint.certhashes.len(), 0);
        assert_eq!(
            endpoint.remote_peer.unwrap(),
            PeerId::from_str("12D3KooWR7EfNv5SLtgjMRjUwR8AvNu3hP4fLrtSa9fmHHXKYWNG").unwrap()
        );
    }

    #[test]
    fn ipv6_webtransport() {
        let addr = Multiaddr::from_str("/ip6/::1/udp/44874/quic-v1/webtransport/certhash/uEiCaDd1Ca1A8IVJ3hsIxIyi11cwxaDKqzVrBkGJbKZU5ng/certhash/uEiDv-VGW8oXxui_G_Kqp-87YjvET-Hr2qYAMYPePJDcsjQ/p2p/12D3KooWR7EfNv5SLtgjMRjUwR8AvNu3hP4fLrtSa9fmHHXKYWNG").unwrap();
        let endpoint = Endpoint::from_multiaddr(&addr).unwrap();

        assert_eq!(endpoint.host, "::1");
        assert_eq!(endpoint.port, 44874);
        assert_eq!(
            endpoint.url(),
            "https://[::1]:44874/.well-known/libp2p-webtransport?type=noise"
        );
    }

    #[test]
    fn dns_webtransport() {
        let addr = Multiaddr::from_str("/dns/libp2p.io/udp/44874/quic-v1/webtransport/certhash/uEiCaDd1Ca1A8IVJ3hsIxIyi11cwxaDKqzVrBkGJbKZU5ng/certhash/uEiDv-VGW8oXxui_G_Kqp-87YjvET-Hr2qYAMYPePJDcsjQ/p2p/12D3KooWR7EfNv5SLtgjMRjUwR8AvNu3hP4fLrtSa9fmHHXKYWNG").unwrap();
        let endpoint = Endpoint::from_multiaddr(&addr).unwrap();

        assert_eq!(endpoint.host, "libp2p.io");
        assert_eq!(endpoint.port, 44874);
        assert_eq!(
            endpoint.url(),
            "https://libp2p.io:44874/.well-known/libp2p-webtransport?type=noise"
        );
    }
}
