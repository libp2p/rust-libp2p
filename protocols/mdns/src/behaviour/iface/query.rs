// Copyright 2018 Parity Technologies (UK) Ltd.
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

use std::{
    fmt,
    net::SocketAddr,
    str,
    time::{Duration, Instant},
};

use hickory_proto::{
    op::Message,
    rr::{Name, RData},
};
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use libp2p_identity::PeerId;
use libp2p_swarm::_address_translation;

use super::dns;
use crate::{META_QUERY_SERVICE_FQDN, SERVICE_NAME_FQDN};

/// A valid mDNS packet received by the service.
#[derive(Debug)]
pub(crate) enum MdnsPacket {
    /// A query made by a remote.
    Query(MdnsQuery),
    /// A response sent by a remote in response to one of our queries.
    Response(MdnsResponse),
    /// A request for service discovery.
    ServiceDiscovery(MdnsServiceDiscovery),
}

// Use a reasonable maximum - 1 day should be sufficient for most scenarios
const MAX_TTL: Duration = Duration::from_secs(60 * 60 * 24);

impl MdnsPacket {
    pub(crate) fn new_from_bytes(
        buf: &[u8],
        from: SocketAddr,
    ) -> Result<Option<MdnsPacket>, hickory_proto::ProtoError> {
        let packet = Message::from_vec(buf)?;

        if packet.query().is_none() {
            return Ok(Some(MdnsPacket::Response(MdnsResponse::new(&packet, from))));
        }

        if packet
            .queries()
            .iter()
            .any(|q| q.name().to_utf8() == SERVICE_NAME_FQDN)
        {
            return Ok(Some(MdnsPacket::Query(MdnsQuery {
                from,
                query_id: packet.header().id(),
            })));
        }

        if packet
            .queries()
            .iter()
            .any(|q| q.name().to_utf8() == META_QUERY_SERVICE_FQDN)
        {
            // TODO: what if multiple questions,
            // one with SERVICE_NAME and one with META_QUERY_SERVICE?
            return Ok(Some(MdnsPacket::ServiceDiscovery(MdnsServiceDiscovery {
                from,
                query_id: packet.header().id(),
            })));
        }

        Ok(None)
    }
}

/// A received mDNS query.
pub(crate) struct MdnsQuery {
    /// Sender of the address.
    from: SocketAddr,
    /// Id of the received DNS query. We need to pass this ID back in the results.
    query_id: u16,
}

impl MdnsQuery {
    /// Source address of the packet.
    pub(crate) fn remote_addr(&self) -> &SocketAddr {
        &self.from
    }

    /// Query id of the packet.
    pub(crate) fn query_id(&self) -> u16 {
        self.query_id
    }
}

impl fmt::Debug for MdnsQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MdnsQuery")
            .field("from", self.remote_addr())
            .field("query_id", &self.query_id)
            .finish()
    }
}

/// A received mDNS service discovery query.
pub(crate) struct MdnsServiceDiscovery {
    /// Sender of the address.
    from: SocketAddr,
    /// Id of the received DNS query. We need to pass this ID back in the results.
    query_id: u16,
}

impl MdnsServiceDiscovery {
    /// Source address of the packet.
    pub(crate) fn remote_addr(&self) -> &SocketAddr {
        &self.from
    }

    /// Query id of the packet.
    pub(crate) fn query_id(&self) -> u16 {
        self.query_id
    }
}

impl fmt::Debug for MdnsServiceDiscovery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MdnsServiceDiscovery")
            .field("from", self.remote_addr())
            .field("query_id", &self.query_id)
            .finish()
    }
}

/// A received mDNS response.
pub(crate) struct MdnsResponse {
    peers: Vec<MdnsPeer>,
    from: SocketAddr,
}

impl MdnsResponse {
    /// Creates a new `MdnsResponse` based on the provided `Packet`.
    pub(crate) fn new(packet: &Message, from: SocketAddr) -> MdnsResponse {
        let peers = packet
            .answers()
            .iter()
            .filter_map(|record| {
                if record.name().to_string() != SERVICE_NAME_FQDN {
                    return None;
                }

                let RData::PTR(record_value) = record.data() else {
                    return None;
                };

                MdnsPeer::new(packet, record_value, record.ttl())
            })
            .collect();

        MdnsResponse { peers, from }
    }

    pub(crate) fn extract_discovered(
        &self,
        now: Instant,
        local_peer_id: PeerId,
    ) -> impl Iterator<Item = (PeerId, Multiaddr, Instant)> + '_ {
        self.discovered_peers()
            .filter(move |peer| peer.id() != &local_peer_id)
            .flat_map(move |peer| {
                let observed = self.observed_address();
                // Cap expiration time to avoid overflow
                let new_expiration = match now.checked_add(peer.ttl()) {
                    Some(expiration) => expiration,
                    None => {
                        now.checked_add(MAX_TTL).unwrap_or(now) // Fallback to now if even that overflows (extremely unlikely)
                    }
                };

                peer.addresses().iter().filter_map(move |address| {
                    let new_addr = _address_translation(address, &observed)?;
                    let new_addr = new_addr.with_p2p(*peer.id()).ok()?;
                    Some((*peer.id(), new_addr, new_expiration))
                })
            })
    }

    /// Source address of the packet.
    pub(crate) fn remote_addr(&self) -> &SocketAddr {
        &self.from
    }

    fn observed_address(&self) -> Multiaddr {
        // We replace the IP address with the address we observe the
        // remote as and the address they listen on.
        let obs_ip = Protocol::from(self.remote_addr().ip());
        let obs_port = Protocol::Udp(self.remote_addr().port());

        Multiaddr::empty().with(obs_ip).with(obs_port)
    }

    /// Returns the list of peers that have been reported in this packet.
    ///
    /// > **Note**: Keep in mind that this will also contain the responses we sent ourselves.
    fn discovered_peers(&self) -> impl Iterator<Item = &MdnsPeer> {
        self.peers.iter()
    }
}

impl fmt::Debug for MdnsResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MdnsResponse")
            .field("from", self.remote_addr())
            .finish()
    }
}

/// A peer discovered by the service.
pub(crate) struct MdnsPeer {
    addrs: Vec<Multiaddr>,
    /// Id of the peer.
    peer_id: PeerId,
    /// TTL of the record in seconds.
    ttl: u32,
}

impl MdnsPeer {
    /// Creates a new `MdnsPeer` based on the provided `Packet`.
    pub(crate) fn new(packet: &Message, record_value: &Name, ttl: u32) -> Option<MdnsPeer> {
        let mut my_peer_id: Option<PeerId> = None;
        let addrs = packet
            .additionals()
            .iter()
            .filter_map(|add_record| {
                if add_record.name() != record_value {
                    return None;
                }

                if let RData::TXT(ref txt) = add_record.data() {
                    Some(txt)
                } else {
                    None
                }
            })
            .flat_map(|txt| txt.iter())
            .filter_map(|txt| {
                // TODO: wrong, txt can be multiple character strings
                let addr = dns::decode_character_string(txt).ok()?;

                if !addr.starts_with(b"dnsaddr=") {
                    return None;
                }

                let mut addr = str::from_utf8(&addr[8..]).ok()?.parse::<Multiaddr>().ok()?;

                match addr.pop() {
                    Some(Protocol::P2p(peer_id)) => {
                        if let Some(pid) = &my_peer_id {
                            if peer_id != *pid {
                                return None;
                            }
                        } else {
                            my_peer_id.replace(peer_id);
                        }
                    }
                    _ => return None,
                };
                Some(addr)
            })
            .collect();

        my_peer_id.map(|peer_id| MdnsPeer {
            addrs,
            peer_id,
            ttl,
        })
    }

    /// Returns the id of the peer.
    #[inline]
    pub(crate) fn id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Returns the requested time-to-live for the record.
    #[inline]
    pub(crate) fn ttl(&self) -> Duration {
        Duration::from_secs(u64::from(self.ttl))
    }

    /// Returns the list of addresses the peer says it is listening on.
    ///
    /// Filters out invalid addresses.
    pub(crate) fn addresses(&self) -> &Vec<Multiaddr> {
        &self.addrs
    }
}

impl fmt::Debug for MdnsPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MdnsPeer")
            .field("peer_id", &self.peer_id)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::{super::dns::build_query_response, *};

    #[test]
    fn test_create_mdns_peer() {
        let ttl = 300;
        let peer_id = PeerId::random();

        let mut addr1: Multiaddr = "/ip4/1.2.3.4/tcp/5000".parse().expect("bad multiaddress");
        let mut addr2: Multiaddr = "/ip6/::1/udp/10000".parse().expect("bad multiaddress");
        addr1.push(Protocol::P2p(peer_id));
        addr2.push(Protocol::P2p(peer_id));

        let packets = build_query_response(
            0xf8f8,
            peer_id,
            vec![&addr1, &addr2].into_iter(),
            Duration::from_secs(60),
        );

        for bytes in packets {
            let packet = Message::from_vec(&bytes).expect("unable to parse packet");
            let record_value = packet
                .answers()
                .iter()
                .filter_map(|record| {
                    if record.name().to_utf8() != SERVICE_NAME_FQDN {
                        return None;
                    }
                    let RData::PTR(record_value) = record.data() else {
                        return None;
                    };
                    Some(record_value)
                })
                .next()
                .expect("empty record value");

            let peer = MdnsPeer::new(&packet, record_value, ttl).expect("fail to create peer");
            assert_eq!(peer.peer_id, peer_id);
        }
    }

    #[test]
    fn test_extract_discovered_ttl_overflow() {
        // Create a mock MdnsResponse with peers that have various TTLs
        let socket_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)), 5353);
        let mut response = MdnsResponse {
            peers: Vec::new(),
            from: socket_addr,
        };

        // Create local peer ID (to be filtered out)
        let local_peer_id = PeerId::random();

        // Create remote peer IDs
        let normal_peer_id = PeerId::random();
        let huge_ttl_peer_id = PeerId::random();

        // Setup addresses
        let addr1 = "/ip4/192.168.1.2/tcp/1234".parse::<Multiaddr>().unwrap();
        let addr2 = "/ip4/192.168.1.3/tcp/5678".parse::<Multiaddr>().unwrap();

        // Add a peer with normal TTL
        response.peers.push(MdnsPeer {
            addrs: vec![addr1],
            peer_id: normal_peer_id,
            ttl: 120, // 2 minutes
        });

        // Add a peer with extremely large TTL
        response.peers.push(MdnsPeer {
            addrs: vec![addr2],
            peer_id: huge_ttl_peer_id,
            ttl: u32::MAX, // Maximum possible TTL
        });

        // Add the local peer (should be filtered out)
        response.peers.push(MdnsPeer {
            addrs: vec!["/ip4/127.0.0.1/tcp/9000".parse::<Multiaddr>().unwrap()],
            peer_id: local_peer_id,
            ttl: 120,
        });

        // Current time
        let now = Instant::now();

        // Call the method under test
        let result: Vec<(PeerId, Multiaddr, Instant)> =
            response.extract_discovered(now, local_peer_id).collect();

        // Verify results
        // Local peer should be filtered out
        assert!(!result.iter().any(|(id, _, _)| id == &local_peer_id));

        // The normal TTL peer should be included
        assert!(result.iter().any(|(id, _, _)| id == &normal_peer_id));

        // The huge TTL peer should now ALWAYS be included
        assert!(result.iter().any(|(id, _, _)| id == &huge_ttl_peer_id));

        // For normal peer, verify expiration time calculation
        let normal_peer_result = result.iter().find(|(id, _, _)| id == &normal_peer_id);
        assert!(normal_peer_result.is_some());

        let (_, _, expiration) = normal_peer_result.unwrap();
        let expected_expiration = now + Duration::from_secs(120);
        assert_eq!(*expiration, expected_expiration);

        // For huge TTL peer, verify expiration time is properly capped
        let huge_ttl_peer_result = result.iter().find(|(id, _, _)| id == &huge_ttl_peer_id);
        assert!(huge_ttl_peer_result.is_some());
        let (_, _, huge_expiration) = huge_ttl_peer_result.unwrap();

        // Check if the TTL would cause overflow
        let huge_ttl_duration = Duration::from_secs(u64::from(u32::MAX));
        let overflow_check = now.checked_add(huge_ttl_duration);

        if overflow_check.is_some() {
            // If no overflow, should be exact expiration
            assert_eq!(*huge_expiration, overflow_check.unwrap());
        } else {
            // If overflow would occur, should be capped
            let one_day = Duration::from_secs(24 * 60 * 60);
            let capped_expiration = now.checked_add(one_day).unwrap_or(now);

            // The expiration should be capped at max 1 day
            assert!(
                *huge_expiration <= capped_expiration,
                "Huge TTL expiration should be capped to prevent overflow"
            );

            // And it should be greater than now (not reset to current time)
            assert!(
                *huge_expiration > now,
                "Capped expiration should be greater than current time"
            );
        }
    }
}
