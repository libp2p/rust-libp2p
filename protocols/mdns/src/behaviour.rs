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

use crate::service::{MdnsPacket, MdnsService, build_query_response, build_service_discovery_response};
use async_io::Timer;
use futures::prelude::*;
use libp2p_core::{
    Multiaddr,
    PeerId,
    address_translation,
    connection::ConnectionId,
    multiaddr::Protocol
};
use libp2p_swarm::{
    NetworkBehaviour,
    NetworkBehaviourAction,
    PollParameters,
    ProtocolsHandler,
    protocols_handler::DummyProtocolsHandler
};
use smallvec::SmallVec;
use std::{cmp, fmt, io, iter, mem, pin::Pin, time::{Duration, Instant}, task::Context, task::Poll};

const MDNS_RESPONSE_TTL: std::time::Duration = Duration::from_secs(5 * 60);

/// A `NetworkBehaviour` for mDNS. Automatically discovers peers on the local network and adds
/// them to the topology.
pub struct Mdns {
    /// The inner service.
    service: MdnsBusyWrapper,

    /// List of nodes that we have discovered, the address, and when their TTL expires.
    ///
    /// Each combination of `PeerId` and `Multiaddr` can only appear once, but the same `PeerId`
    /// can appear multiple times.
    discovered_nodes: SmallVec<[(PeerId, Multiaddr, Instant); 8]>,

    /// Future that fires when the TTL of at least one node in `discovered_nodes` expires.
    ///
    /// `None` if `discovered_nodes` is empty.
    closest_expiration: Option<Timer>,
}

/// `MdnsService::next` takes ownership of `self`, returning a future that resolves with both itself
/// and a `MdnsPacket` (similar to the old Tokio socket send style). The two states are thus `Free`
/// with an `MdnsService` or `Busy` with a future returning the original `MdnsService` and an
/// `MdnsPacket`.
enum MdnsBusyWrapper {
    Free(MdnsService),
    Busy(Pin<Box<dyn Future<Output = (MdnsService, MdnsPacket)> + Send>>),
    Poisoned,
}

impl fmt::Debug for MdnsBusyWrapper {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Free(service) => {
                fmt.debug_struct("MdnsBusyWrapper::Free")
                    .field("service", service)
                    .finish()
            },
            Self::Busy(_) => {
                fmt.debug_struct("MdnsBusyWrapper::Busy")
                    .finish()
            }
            Self::Poisoned => {
                fmt.debug_struct("MdnsBusyWrapper::Poisoned")
                    .finish()
            }
        }
    }
}

impl Mdns {
    /// Builds a new `Mdns` behaviour.
    pub async fn new() -> io::Result<Self> {
        Ok(Self {
            service: MdnsBusyWrapper::Free(MdnsService::new().await?),
            discovered_nodes: SmallVec::new(),
            closest_expiration: None,
        })
    }

    /// Returns true if the given `PeerId` is in the list of nodes discovered through mDNS.
    pub fn has_node(&self, peer_id: &PeerId) -> bool {
        self.discovered_nodes().any(|p| p == peer_id)
    }

    /// Returns the list of nodes that we have discovered through mDNS and that are not expired.
    pub fn discovered_nodes(&self) -> impl ExactSizeIterator<Item = &PeerId> {
        self.discovered_nodes.iter().map(|(p, _, _)| p)
    }
}

impl NetworkBehaviour for Mdns {
    type ProtocolsHandler = DummyProtocolsHandler;
    type OutEvent = MdnsEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        DummyProtocolsHandler::default()
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        let now = Instant::now();
        self.discovered_nodes
            .iter()
            .filter(move |(p, _, expires)| p == peer_id && *expires > now)
            .map(|(_, addr, _)| addr.clone())
            .collect()
    }

    fn inject_connected(&mut self, _: &PeerId) {}

    fn inject_disconnected(&mut self, _: &PeerId) {}

    fn inject_event(
        &mut self,
        _: PeerId,
        _: ConnectionId,
        ev: <Self::ProtocolsHandler as ProtocolsHandler>::OutEvent,
    ) {
        void::unreachable(ev)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        // Remove expired peers.
        if let Some(ref mut closest_expiration) = self.closest_expiration {
            match Pin::new(closest_expiration).poll(cx) {
                Poll::Ready(now) => {
                    let mut expired = SmallVec::<[(PeerId, Multiaddr); 4]>::new();
                    while let Some(pos) = self.discovered_nodes.iter().position(|(_, _, exp)| *exp < now) {
                        let (peer_id, addr, _) = self.discovered_nodes.remove(pos);
                        expired.push((peer_id, addr));
                    }

                    if !expired.is_empty() {
                        let event = MdnsEvent::Expired(ExpiredAddrsIter {
                            inner: expired.into_iter(),
                        });

                        return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
                    }
                },
                Poll::Pending => (),
            }
        }

        // Polling the mDNS service, and obtain the list of nodes discovered this round.
        let discovered = loop {
            let service = mem::replace(&mut self.service, MdnsBusyWrapper::Poisoned);

            let packet = match service {
                MdnsBusyWrapper::Free(service) => {
                    self.service = MdnsBusyWrapper::Busy(Box::pin(service.next()));
                    continue;
                },
                MdnsBusyWrapper::Busy(mut fut) => {
                    match fut.as_mut().poll(cx) {
                        Poll::Ready((service, packet)) => {
                            self.service = MdnsBusyWrapper::Free(service);
                            packet
                        },
                        Poll::Pending => {
                            self.service = MdnsBusyWrapper::Busy(fut);
                            return Poll::Pending;
                        }
                    }
                },
                MdnsBusyWrapper::Poisoned => panic!("Mdns poisoned"),
            };

            match packet {
                MdnsPacket::Query(query) => {
                    // MaybeBusyMdnsService should always be Free.
                    if let MdnsBusyWrapper::Free(ref mut service) = self.service {
                        for packet in build_query_response(
                            query.query_id(),
                            *params.local_peer_id(),
                            params.listened_addresses(),
                            MDNS_RESPONSE_TTL,
                        ) {
                            service.enqueue_response(packet)
                        }
                    } else { debug_assert!(false); }
                },
                MdnsPacket::Response(response) => {
                    // We replace the IP address with the address we observe the
                    // remote as and the address they listen on.
                    let obs_ip = Protocol::from(response.remote_addr().ip());
                    let obs_port = Protocol::Udp(response.remote_addr().port());
                    let observed: Multiaddr = iter::once(obs_ip)
                        .chain(iter::once(obs_port))
                        .collect();

                    let mut discovered: SmallVec<[_; 4]> = SmallVec::new();
                    for peer in response.discovered_peers() {
                        if peer.id() == params.local_peer_id() {
                            continue;
                        }

                        let new_expiration = Instant::now() + peer.ttl();

                        let mut addrs: Vec<Multiaddr> = Vec::new();
                        for addr in peer.addresses() {
                            if let Some(new_addr) = address_translation(&addr, &observed) {
                                addrs.push(new_addr.clone())
                            }
                            addrs.push(addr.clone())
                        }

                        for addr in addrs {
                            if let Some((_, _, cur_expires)) = self.discovered_nodes.iter_mut()
                                .find(|(p, a, _)| p == peer.id() && *a == addr)
                            {
                                *cur_expires = cmp::max(*cur_expires, new_expiration);
                            } else {
                                self.discovered_nodes.push((*peer.id(), addr.clone(), new_expiration));
                            }

                            discovered.push((*peer.id(), addr));
                        }
                    }

                    break discovered;
                },
                MdnsPacket::ServiceDiscovery(disc) => {
                    // MaybeBusyMdnsService should always be Free.
                    if let MdnsBusyWrapper::Free(ref mut service) = self.service {
                        let resp = build_service_discovery_response(
                            disc.query_id(),
                            MDNS_RESPONSE_TTL,
                        );
                        service.enqueue_response(resp);
                    } else { debug_assert!(false); }
                },
            }
        };

        // Getting this far implies that we discovered new nodes. As the final step, we need to
        // refresh `closest_expiration`.
        self.closest_expiration = self.discovered_nodes.iter()
            .fold(None, |exp, &(_, _, elem_exp)| {
                Some(exp.map(|exp| cmp::min(exp, elem_exp)).unwrap_or(elem_exp))
            })
            .map(Timer::at);

        Poll::Ready(NetworkBehaviourAction::GenerateEvent(MdnsEvent::Discovered(DiscoveredAddrsIter {
            inner: discovered.into_iter(),
        })))
    }
}

impl fmt::Debug for Mdns {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("Mdns")
            .field("service", &self.service)
            .finish()
    }
}

/// Event that can be produced by the `Mdns` behaviour.
#[derive(Debug)]
pub enum MdnsEvent {
    /// Discovered nodes through mDNS.
    Discovered(DiscoveredAddrsIter),

    /// The given combinations of `PeerId` and `Multiaddr` have expired.
    ///
    /// Each discovered record has a time-to-live. When this TTL expires and the address hasn't
    /// been refreshed, we remove it from the list and emit it as an `Expired` event.
    Expired(ExpiredAddrsIter),
}

/// Iterator that produces the list of addresses that have been discovered.
pub struct DiscoveredAddrsIter {
    inner: smallvec::IntoIter<[(PeerId, Multiaddr); 4]>
}

impl Iterator for DiscoveredAddrsIter {
    type Item = (PeerId, Multiaddr);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for DiscoveredAddrsIter {
}

impl fmt::Debug for DiscoveredAddrsIter {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("DiscoveredAddrsIter")
            .finish()
    }
}

/// Iterator that produces the list of addresses that have expired.
pub struct ExpiredAddrsIter {
    inner: smallvec::IntoIter<[(PeerId, Multiaddr); 4]>
}

impl Iterator for ExpiredAddrsIter {
    type Item = (PeerId, Multiaddr);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for ExpiredAddrsIter {
}

impl fmt::Debug for ExpiredAddrsIter {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("ExpiredAddrsIter")
            .finish()
    }
}
