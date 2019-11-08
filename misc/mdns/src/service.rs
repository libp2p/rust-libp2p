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

use crate::{SERVICE_NAME, META_QUERY_SERVICE, dns};
use async_std::net::UdpSocket;
use dns_parser::{Packet, RData};
use futures::prelude::*;
use libp2p_core::{Multiaddr, PeerId};
use multiaddr::Protocol;
use std::{fmt, io, net::Ipv4Addr, net::SocketAddr, str, time::{Duration, Instant}};
use wasm_timer::Interval;
use lazy_static::lazy_static;

pub use dns::MdnsResponseError;

lazy_static! {
    static ref IPV4_MDNS_MULTICAST_ADDRESS: SocketAddr = SocketAddr::from((
        Ipv4Addr::new(224, 0, 0, 251),
        5353,
    ));
}

/// A running service that discovers libp2p peers and responds to other libp2p peers' queries on
/// the local network.
///
/// # Usage
///
/// In order to use mDNS to discover peers on the local network, use the `MdnsService`. This is
/// done by creating a `MdnsService` then polling it in the same way as you would poll a stream.
///
/// Polling the `MdnsService` can produce either an `MdnsQuery`, corresponding to an mDNS query
/// received by another node on the local network, or an `MdnsResponse` corresponding to a response
/// to a query previously emitted locally. The `MdnsService` will automatically produce queries,
/// which means that you will receive responses automatically.
///
/// When you receive an `MdnsQuery`, use the `respond` method to send back an answer to the node
/// that emitted the query.
///
/// When you receive an `MdnsResponse`, use the provided methods to query the information received
/// in the response.
///
/// # Example
///
/// ```rust
/// # use futures::prelude::*;
/// # use futures::executor::block_on;
/// # use libp2p_core::{identity, PeerId};
/// # use libp2p_mdns::service::{MdnsService, MdnsPacket};
/// # use std::{io, time::Duration, task::Poll};
/// # fn main() {
/// # let my_peer_id = PeerId::from(identity::Keypair::generate_ed25519().public());
/// # let my_listened_addrs = Vec::new();
/// # block_on(async {
/// let mut service = MdnsService::new().await.expect("Error while creating mDNS service");
/// let _future_to_poll = async {
///     let (mut service, packet) = service.next().await;
///
///     match packet {
///         MdnsPacket::Query(query) => {
///             println!("Query from {:?}", query.remote_addr());
///             service.enqueue_response(query.build_response(
///                 my_peer_id.clone(),
///                 my_listened_addrs.clone(),
///                 Duration::from_secs(120),
///             ).unwrap());
///         }
///         MdnsPacket::Response(response) => {
///             for peer in response.discovered_peers() {
///                 println!("Discovered peer {:?}", peer.id());
///                 for addr in peer.addresses() {
///                     println!("Address = {:?}", addr);
///                 }
///             }
///         }
///         MdnsPacket::ServiceDiscovery(query) => {
///             service.enqueue_response(query.build_response(std::time::Duration::from_secs(120)));
///         }
///     }
/// };
/// # })
/// # }
pub struct MdnsService {
    /// Main socket for listening.
    socket: UdpSocket,
    /// Socket for sending queries on the network.
    query_socket: UdpSocket,
    /// Interval for sending queries.
    query_interval: Interval,
    /// Whether we send queries on the network at all.
    /// Note that we still need to have an interval for querying, as we need to wake up the socket
    /// regularly to recover from errors. Otherwise we could simply use an `Option<Interval>`.
    silent: bool,
    /// Buffer used for receiving data from the main socket.
    recv_buffer: [u8; 2048],
    /// Buffers pending to send on the main socket.
    send_buffers: Vec<Vec<u8>>,
    /// Buffers pending to send on the query socket.
    query_send_buffers: Vec<Vec<u8>>,
}

impl MdnsService {
    /// Starts a new mDNS service.
    #[inline]
    pub async fn new() -> io::Result<MdnsService> {
        Self::new_inner(false).await
    }

    /// Same as `new`, but we don't send automatically send queries on the network.
    #[inline]
    pub async fn silent() -> io::Result<MdnsService> {
        Self::new_inner(true).await
    }

    /// Starts a new mDNS service.
    async fn new_inner(silent: bool) -> io::Result<MdnsService> {
        let socket = {
            #[cfg(unix)]
            fn platform_specific(s: &net2::UdpBuilder) -> io::Result<()> {
                net2::unix::UnixUdpBuilderExt::reuse_port(s, true)?;
                Ok(())
            }
            #[cfg(not(unix))]
            fn platform_specific(_: &net2::UdpBuilder) -> io::Result<()> { Ok(()) }
            let builder = net2::UdpBuilder::new_v4()?;
            builder.reuse_address(true)?;
            platform_specific(&builder)?;
            builder.bind(("0.0.0.0", 5353))?
        };

        let socket = UdpSocket::from(socket);
        socket.set_multicast_loop_v4(true)?;
        socket.set_multicast_ttl_v4(255)?;
        // TODO: correct interfaces?
        socket.join_multicast_v4(&From::from([224, 0, 0, 251]), &Ipv4Addr::UNSPECIFIED)?;

        Ok(MdnsService {
            socket,
            query_socket: UdpSocket::bind((Ipv4Addr::from([0u8, 0, 0, 0]), 0u16)).await?,
            query_interval: Interval::new_at(Instant::now(), Duration::from_secs(20)),
            silent,
            recv_buffer: [0; 2048],
            send_buffers: Vec::new(),
            query_send_buffers: Vec::new(),
        })
    }

    pub fn enqueue_response(&mut self, rsp: Vec<u8>) {
        self.send_buffers.push(rsp);
    }

    /// Returns a future resolving to itself and the next received `MdnsPacket`.
    ///
    /// **Note**: Why does `next` take ownership of itself?
    ///
    /// `MdnsService::next` needs to be called from within `NetworkBehaviour`
    /// implementations. Given that traits can not have async methods the
    /// respective `NetworkBehaviour` implementation needs to somehow keep the
    /// Future returned by `MdnsService::next` across classic `poll`
    /// invocations. The instance method `next` can either take a reference or
    /// ownership of itself:
    ///
    /// 1. Taking a reference - If `MdnsService::poll` takes a reference to
    /// `&self` the respective `NetworkBehaviour` implementation would need to
    /// keep both the Future as well as its `MdnsService` instance across poll
    /// invocations. Given that in this case the Future would have a reference
    /// to `MdnsService`, the `NetworkBehaviour` implementation struct would
    /// need to be self-referential which is not possible without unsafe code in
    /// Rust.
    ///
    /// 2. Taking ownership - Instead `MdnsService::next` takes ownership of
    /// self and returns it alongside an `MdnsPacket` once the actual future
    /// resolves, not forcing self-referential structures on the caller.
    pub async fn next(mut self) -> (Self, MdnsPacket) {
        // Send a query every time `query_interval` fires.
        //
        // Note that we don't use a loop here—it is pretty unlikely that we need it, and there is
        // no point in sending multiple requests in a row.
        if let Some(_) = self.query_interval.next().now_or_never() {
            // Ensure underlying task is woken up on the next interval tick.
            while let Some(_) = self.query_interval.next().now_or_never() {};

            if !self.silent {
                let query = dns::build_query();
                self.query_send_buffers.push(query.to_vec());
            }
        };

        // Flush the send buffer of the main socket.
        while !self.send_buffers.is_empty() {
            let to_send = self.send_buffers.remove(0);

            match self.socket.send_to(&to_send, *IPV4_MDNS_MULTICAST_ADDRESS).await {
                Ok(bytes_written) => {
                    debug_assert_eq!(bytes_written, to_send.len());
                }
                Err(_) => {
                    // Errors are non-fatal because they can happen for example if we lose
                    // connection to the network.
                    self.send_buffers.clear();
                    break;
                }
            }
        }

        // Flush the query send buffer.
        // This has to be after the push to `query_send_buffers`.
        while !self.query_send_buffers.is_empty() {
            let to_send = self.query_send_buffers.remove(0);

            match self.query_socket.send_to(&to_send, *IPV4_MDNS_MULTICAST_ADDRESS).await {
                Ok(bytes_written) => {
                    debug_assert_eq!(bytes_written, to_send.len());
                }
                Err(_) => {
                    // Errors are non-fatal because they can happen for example if we lose
                    // connection to the network.
                    self.query_send_buffers.clear();
                    break;
                }
            }
        }

        loop {
            match self.socket.recv_from(&mut self.recv_buffer).await {
                Ok((len, from)) => {
                    match Packet::parse(&self.recv_buffer[..len]) {
                        Ok(packet) => {
                            if packet.header.query {
                                if packet
                                    .questions
                                    .iter()
                                    .any(|q| q.qname.to_string().as_bytes() == SERVICE_NAME)
                                {
                                    let query = MdnsPacket::Query(MdnsQuery {
                                        from,
                                        query_id: packet.header.id,
                                    });
                                    return (self, query);
                                } else if packet
                                    .questions
                                    .iter()
                                    .any(|q| q.qname.to_string().as_bytes() == META_QUERY_SERVICE)
                                {
                                    // TODO: what if multiple questions, one with SERVICE_NAME and one with META_QUERY_SERVICE?
                                    let discovery = MdnsPacket::ServiceDiscovery(
                                        MdnsServiceDiscovery {
                                            from,
                                            query_id: packet.header.id,
                                        },
                                    );
                                    return (self, discovery);
                                } else {
                                    continue;
                                }
                            } else {
                                let resp = MdnsPacket::Response(MdnsResponse::new (
                                    packet,
                                    from,
                                ));
                                return (self, resp);
                            }
                        }
                        Err(_) => {
                            continue;
                        }
                    }
                }
                Err(_) => {
                    // Error are non-fatal and can happen if we get disconnected from example.
                    // The query interval will wake up the task at some point so that we can try again.
                }
            }};
    }
}

impl fmt::Debug for MdnsService {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("MdnsService")
            .field("silent", &self.silent)
            .finish()
    }
}

/// A valid mDNS packet received by the service.
#[derive(Debug)]
pub enum MdnsPacket {
    /// A query made by a remote.
    Query(MdnsQuery),
    /// A response sent by a remote in response to one of our queries.
    Response(MdnsResponse),
    /// A request for service discovery.
    ServiceDiscovery(MdnsServiceDiscovery),
}

/// A received mDNS query.
pub struct MdnsQuery {
    /// Sender of the address.
    from: SocketAddr,
    /// Id of the received DNS query. We need to pass this ID back in the results.
    query_id: u16,
}

impl MdnsQuery {
    /// Build query response.
    ///
    /// Pass the ID of the local peer, and the list of addresses we're listening on.
    ///
    /// If there are more than 2^16-1 addresses, ignores the others.
    ///
    /// > **Note**: Keep in mind that we will also receive this response in an `MdnsResponse`.
    #[inline]
    pub fn build_response<TAddresses>(
        self,
        peer_id: PeerId,
        addresses: TAddresses,
        ttl: Duration,
    ) -> Result<Vec<u8>, MdnsResponseError>
    where
        TAddresses: IntoIterator<Item = Multiaddr>,
        TAddresses::IntoIter: ExactSizeIterator,
    {
        dns::build_query_response(self.query_id, peer_id, addresses.into_iter(), ttl)
    }

    /// Source address of the packet.
    #[inline]
    pub fn remote_addr(&self) -> &SocketAddr {
        &self.from
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
pub struct MdnsServiceDiscovery {
    /// Sender of the address.
    from: SocketAddr,
    /// Id of the received DNS query. We need to pass this ID back in the results.
    query_id: u16,
}

impl MdnsServiceDiscovery {
    /// Build service discovery response.
    #[inline]
    pub fn build_response(self, ttl: Duration) -> Vec<u8> {
        dns::build_service_discovery_response(self.query_id, ttl)
    }

    /// Source address of the packet.
    #[inline]
    pub fn remote_addr(&self) -> &SocketAddr {
        &self.from
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
pub struct MdnsResponse {
    peers: Vec<MdnsPeer>,
    from: SocketAddr,
}

impl MdnsResponse {
    /// Creates a new `MdnsResponse` based on the provided `Packet`.
    fn new(packet: Packet, from: SocketAddr) -> MdnsResponse {
        let peers = packet.answers.iter().filter_map(|record| {
            if record.name.to_string().as_bytes() != SERVICE_NAME {
                return None;
            }

            let record_value = match record.data {
                RData::PTR(record) => record.0.to_string(),
                _ => return None,
            };

            let peer_name = {
                let mut iter = record_value.splitn(2, |c| c == '.');
                let name = match iter.next() {
                    Some(n) => n.to_owned(),
                    None => return None,
                };
                if iter.next().map(|v| v.as_bytes()) != Some(SERVICE_NAME) {
                    return None;
                }
                name
            };

            let peer_id = match data_encoding::BASE32_DNSCURVE.decode(peer_name.as_bytes()) {
                Ok(bytes) => match PeerId::from_bytes(bytes) {
                    Ok(id) => id,
                    Err(_) => return None,
                },
                Err(_) => return None,
            };

            Some(MdnsPeer::new (
                &packet,
                record_value,
                peer_id,
                record.ttl,
            ))
        }).collect();

        MdnsResponse {
            peers,
            from,
        }
    }

    /// Returns the list of peers that have been reported in this packet.
    ///
    /// > **Note**: Keep in mind that this will also contain the responses we sent ourselves.
    pub fn discovered_peers(&self) -> &Vec<MdnsPeer> {
        &self.peers
    }

    /// Source address of the packet.
    #[inline]
    pub fn remote_addr(&self) -> &SocketAddr {
        &self.from
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
pub struct MdnsPeer {
    addrs: Vec<Multiaddr>,
    /// Id of the peer.
    peer_id: PeerId,
    /// TTL of the record in seconds.
    ttl: u32,
}

impl MdnsPeer {
    /// Creates a new `MdnsPeer` based on the provided `Packet`.
    pub fn new(packet: &Packet, record_value: String, my_peer_id: PeerId, ttl: u32) -> MdnsPeer {
        let addrs = packet
            .additional
            .iter()
            .filter_map(|add_record| {
                if add_record.name.to_string() != record_value {
                    return None;
                }

                if let RData::TXT(ref txt) = add_record.data {
                    Some(txt)
                } else {
                    None
                }
            })
            .flat_map(|txt| txt.iter())
            .filter_map(|txt| {
                // TODO: wrong, txt can be multiple character strings
                let addr = match dns::decode_character_string(txt) {
                    Ok(a) => a,
                    Err(_) => return None,
                };
                if !addr.starts_with(b"dnsaddr=") {
                    return None;
                }
                let addr = match str::from_utf8(&addr[8..]) {
                    Ok(a) => a,
                    Err(_) => return None,
                };
                let mut addr = match addr.parse::<Multiaddr>() {
                    Ok(a) => a,
                    Err(_) => return None,
                };
                match addr.pop() {
                    Some(Protocol::P2p(ref peer_id)) if peer_id == &my_peer_id => (),
                    _ => return None,
                };
                Some(addr)
            }).collect();

        MdnsPeer {
            addrs,
            peer_id: my_peer_id.clone(),
            ttl,
        }
    }

    /// Returns the id of the peer.
    #[inline]
    pub fn id(&self) -> &PeerId {
        &self.peer_id
    }

    /// Returns the requested time-to-live for the record.
    #[inline]
    pub fn ttl(&self) -> Duration {
        Duration::from_secs(u64::from(self.ttl))
    }

    /// Returns the list of addresses the peer says it is listening on.
    ///
    /// Filters out invalid addresses.
    pub fn addresses(&self) -> &Vec<Multiaddr> {
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
    use futures::executor::block_on;
    use libp2p_core::PeerId;
    use std::time::Duration;
    use crate::service::{MdnsPacket, MdnsService};

    #[test]
    fn discover_ourselves() {
        block_on (async {
            let mut service = MdnsService::new().await.unwrap();
            let peer_id = PeerId::random();

            loop {
                let next = service.next().await;
                service = next.0;

                match next.1 {
                    MdnsPacket::Query(query) => {
                        service.enqueue_response(query.build_response(peer_id.clone(), None, Duration::from_secs(120)).unwrap());
                    }
                    MdnsPacket::Response(response) => {
                        for peer in response.discovered_peers() {
                            if peer.id() == &peer_id {
                                return;
                            }
                        }
                    }
                    MdnsPacket::ServiceDiscovery(_) => {panic!("did not expect a service discovery packet")}
                }
            }
        })
    }
}
