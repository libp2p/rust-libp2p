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

mod dns;
mod query;

use self::dns::{build_query, build_query_response, build_service_discovery_response};
use self::query::MdnsPacket;
use crate::behaviour::iface::query::MdnsResponse;
use crate::behaviour::{socket::AsyncSocket, timer::Builder};
use crate::MdnsConfig;
use libp2p_core::{address_translation, Multiaddr, PeerId};
use libp2p_swarm::PollParameters;
use socket2::{Domain, Socket, Type};
use std::{
    collections::VecDeque,
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, Instant},
};

/// An mDNS instance for a networking interface. To discover all peers when having multiple
/// interfaces an [`InterfaceState`] is required for each interface.
#[derive(Debug)]
pub struct InterfaceState<U, T> {
    /// Address this instance is bound to.
    addr: IpAddr,
    /// Receive socket.
    recv_socket: U,
    /// Send socket.
    send_socket: U,
    /// Buffer used for receiving data from the main socket.
    /// RFC6762 discourages packets larger than the interface MTU, but allows sizes of up to 9000
    /// bytes, if it can be ensured that all participating devices can handle such large packets.
    /// For computers with several interfaces and IP addresses responses can easily reach sizes in
    /// the range of 3000 bytes, so 4096 seems sensible for now. For more information see
    /// [rfc6762](https://tools.ietf.org/html/rfc6762#page-46).
    recv_buffer: [u8; 4096],
    /// Buffers pending to send on the main socket.
    send_buffer: VecDeque<Vec<u8>>,
    /// Discovery interval.
    query_interval: Duration,
    /// Discovery timer.
    timeout: T,
    /// Multicast address.
    multicast_addr: IpAddr,
    /// Discovered addresses.
    discovered: VecDeque<(PeerId, Multiaddr, Instant)>,
    /// TTL
    ttl: Duration,
}

impl<U, T> InterfaceState<U, T>
where
    U: AsyncSocket,
    T: Builder + futures::Stream,
{
    /// Builds a new [`InterfaceState`].
    pub fn new(addr: IpAddr, config: MdnsConfig) -> io::Result<Self> {
        log::info!("creating instance on iface {}", addr);
        let recv_socket = match addr {
            IpAddr::V4(addr) => {
                let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(socket2::Protocol::UDP))?;
                socket.set_reuse_address(true)?;
                #[cfg(unix)]
                socket.set_reuse_port(true)?;
                socket.bind(&SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 5353).into())?;
                socket.set_multicast_loop_v4(true)?;
                socket.set_multicast_ttl_v4(255)?;
                socket.join_multicast_v4(&crate::IPV4_MDNS_MULTICAST_ADDRESS, &addr)?;
                U::from_std(UdpSocket::from(socket))?
            }
            IpAddr::V6(_) => {
                let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(socket2::Protocol::UDP))?;
                socket.set_reuse_address(true)?;
                #[cfg(unix)]
                socket.set_reuse_port(true)?;
                socket.bind(&SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 5353).into())?;
                socket.set_multicast_loop_v6(true)?;
                // TODO: find interface matching addr.
                socket.join_multicast_v6(&crate::IPV6_MDNS_MULTICAST_ADDRESS, 0)?;
                U::from_std(UdpSocket::from(socket))?
            }
        };
        let bind_addr = match addr {
            IpAddr::V4(_) => SocketAddr::new(addr, 0),
            IpAddr::V6(_addr) => {
                // TODO: if-watch should return the scope_id of an address
                // as a workaround we bind to unspecified, which means that
                // this probably won't work when using multiple interfaces.
                // SocketAddr::V6(SocketAddrV6::new(addr, 0, 0, scope_id))
                SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)
            }
        };
        let send_socket = U::from_std(UdpSocket::bind(bind_addr)?)?;

        // randomize timer to prevent all converging and firing at the same time.
        let query_interval = {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            let jitter = rng.gen_range(0..100);
            config.query_interval + Duration::from_millis(jitter)
        };
        let multicast_addr = match addr {
            IpAddr::V4(_) => IpAddr::V4(*crate::IPV4_MDNS_MULTICAST_ADDRESS),
            IpAddr::V6(_) => IpAddr::V6(*crate::IPV6_MDNS_MULTICAST_ADDRESS),
        };
        Ok(Self {
            addr,
            recv_socket,
            send_socket,
            recv_buffer: [0; 4096],
            send_buffer: Default::default(),
            discovered: Default::default(),
            query_interval,
            timeout: T::interval_at(Instant::now(), query_interval),
            multicast_addr,
            ttl: config.ttl,
        })
    }

    pub fn reset_timer(&mut self) {
        self.timeout = T::interval(self.query_interval);
    }

    pub fn fire_timer(&mut self) {
        self.timeout = T::interval_at(Instant::now(), self.query_interval);
    }

    pub fn poll(
        &mut self,
        cx: &mut Context,
        params: &impl PollParameters,
    ) -> Poll<(PeerId, Multiaddr, Instant)> {
        loop {
            // 1st priority: Low latency: Create packet ASAP after timeout.
            if Pin::new(&mut self.timeout).poll_next(cx).is_ready() {
                log::trace!("sending query on iface {}", self.addr);
                self.send_buffer.push_back(build_query());
            }

            // 2nd priority: Keep local buffers small: Send packets to remote.
            if let Some(packet) = self.send_buffer.pop_front() {
                match Pin::new(&mut self.send_socket).poll_write(
                    cx,
                    &packet,
                    SocketAddr::new(self.multicast_addr, 5353),
                ) {
                    Poll::Ready(Ok(_)) => {
                        log::trace!("sent packet on iface {}", self.addr);
                        continue;
                    }
                    Poll::Ready(Err(err)) => {
                        log::error!("error sending packet on iface {} {}", self.addr, err);
                        continue;
                    }
                    Poll::Pending => {
                        self.send_buffer.push_front(packet);
                    }
                }
            }

            // 3rd priority: Keep local buffers small: Return discovered addresses.
            if let Some(discovered) = self.discovered.pop_front() {
                return Poll::Ready(discovered);
            }

            // 4th priority: Remote work: Answer incoming requests.
            match Pin::new(&mut self.recv_socket)
                .poll_read(cx, &mut self.recv_buffer)
                .map_ok(|(len, from)| MdnsPacket::new_from_bytes(&self.recv_buffer[..len], from))
            {
                Poll::Ready(Ok(Some(MdnsPacket::Query(query)))) => {
                    self.reset_timer();
                    log::trace!("sending response on iface {}", self.addr);

                    self.send_buffer.extend(build_query_response(
                        query.query_id(),
                        *params.local_peer_id(),
                        params.listened_addresses(),
                        self.ttl,
                    ));
                    continue;
                }
                Poll::Ready(Ok(Some(MdnsPacket::Response(response)))) => {
                    self.discovered
                        .extend(extract_discovered(&response, params.local_peer_id()));
                    continue;
                }
                Poll::Ready(Ok(Some(MdnsPacket::ServiceDiscovery(disc)))) => {
                    self.send_buffer
                        .push_back(build_service_discovery_response(disc.query_id(), self.ttl));
                    continue;
                }
                Poll::Ready(Err(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    // No more bytes available on the socket to read
                }
                Poll::Ready(Ok(None)) | Poll::Pending => {}
                Poll::Ready(Err(err)) => {
                    log::error!("failed reading datagram: {}", err);
                }
            }

            return Poll::Pending;
        }
    }
}

fn extract_discovered<'r>(
    response: &'r MdnsResponse,
    local_peer_id: &'r PeerId,
) -> impl Iterator<Item = (PeerId, Multiaddr, Instant)> + 'r {
    response
        .discovered_peers()
        .filter(move |peer| peer.id() != local_peer_id)
        .flat_map(|peer| {
            let observed = response.observed_address();
            let new_expiration = Instant::now() + peer.ttl();

            peer.addresses().iter().filter_map(move |address| {
                let new_addr = address_translation(address, &observed)?;

                Some((*peer.id(), new_addr.clone(), new_expiration))
            })
        })
}
