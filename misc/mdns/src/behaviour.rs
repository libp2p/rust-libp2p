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

use crate::service::{MdnsService, MdnsPacket};
use futures::prelude::*;
use libp2p_core::{address_translation, ConnectedPoint, Multiaddr, PeerId, multiaddr::Protocol};
use libp2p_swarm::{
    NetworkBehaviour,
    NetworkBehaviourAction,
    PollParameters,
    ProtocolsHandler,
    protocols_handler::DummyProtocolsHandler
};
use log::warn;
use smallvec::SmallVec;
use std::{cmp, fmt, io, iter, marker::PhantomData, time::Duration};
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_timer::{Delay, Instant};

/// A `NetworkBehaviour` for mDNS. Automatically discovers peers on the local network and adds
/// them to the topology.
pub struct Mdns<TSubstream> {
    /// The inner service.
    service: MdnsService,

    /// List of nodes that we have discovered, the address, and when their TTL expires.
    ///
    /// Each combination of `PeerId` and `Multiaddr` can only appear once, but the same `PeerId`
    /// can appear multiple times.
    discovered_nodes: SmallVec<[(PeerId, Multiaddr, Instant); 8]>,

    /// Future that fires when the TTL at least one node in `discovered_nodes` expires.
    ///
    /// `None` if `discovered_nodes` is empty.
    closest_expiration: Option<Delay>,

    /// Marker to pin the generic.
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> Mdns<TSubstream> {
    /// Builds a new `Mdns` behaviour.
    pub fn new() -> io::Result<Mdns<TSubstream>> {
        Ok(Mdns {
            service: MdnsService::new()?,
            discovered_nodes: SmallVec::new(),
            closest_expiration: None,
            marker: PhantomData,
        })
    }

    /// Returns true if the given `PeerId` is in the list of nodes discovered through mDNS.
    pub fn has_node(&self, peer_id: &PeerId) -> bool {
        self.discovered_nodes.iter().any(|(p, _, _)| p == peer_id)
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
    /// been refreshed, we remove it from the list emit it as an `Expired` event.
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

impl<TSubstream> NetworkBehaviour for Mdns<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = DummyProtocolsHandler<TSubstream>;
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

    fn inject_connected(&mut self, _: PeerId, _: ConnectedPoint) {}

    fn inject_disconnected(&mut self, _: &PeerId, _: ConnectedPoint) {}

    fn inject_node_event(
        &mut self,
        _: PeerId,
        _ev: <Self::ProtocolsHandler as ProtocolsHandler>::OutEvent,
    ) {
        void::unreachable(_ev)
    }

    fn poll(
        &mut self,
        params: &mut impl PollParameters,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        // Remove expired peers.
        if let Some(ref mut closest_expiration) = self.closest_expiration {
            match closest_expiration.poll() {
                Ok(Async::Ready(())) => {
                    let now = Instant::now();
                    let mut expired = SmallVec::<[(PeerId, Multiaddr); 4]>::new();
                    while let Some(pos) = self.discovered_nodes.iter().position(|(_, _, exp)| *exp < now) {
                        let (peer_id, addr, _) = self.discovered_nodes.remove(pos);
                        expired.push((peer_id, addr));
                    }

                    if !expired.is_empty() {
                        let event = MdnsEvent::Expired(ExpiredAddrsIter {
                            inner: expired.into_iter(),
                        });

                        return Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
                    }
                },
                Ok(Async::NotReady) => (),
                Err(err) => warn!("tokio timer has errored: {:?}", err),
            }
        }

        // Polling the mDNS service, and obtain the list of nodes discovered this round.
        let discovered = loop {
            let event = match self.service.poll() {
                Async::Ready(ev) => ev,
                Async::NotReady => return Async::NotReady,
            };

            match event {
                MdnsPacket::Query(query) => {
                    let _ = query.respond(
                        params.local_peer_id().clone(),
                        params.listened_addresses(),
                        Duration::from_secs(5 * 60)
                    );
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

                        let mut addrs = Vec::new();
                        for addr in peer.addresses() {
                            if let Some(new_addr) = address_translation(&addr, &observed) {
                                addrs.push(new_addr)
                            }
                            addrs.push(addr)
                        }

                        for addr in addrs {
                            if let Some((_, _, cur_expires)) = self.discovered_nodes.iter_mut()
                                .find(|(p, a, _)| p == peer.id() && *a == addr)
                            {
                                *cur_expires = cmp::max(*cur_expires, new_expiration);
                            } else {
                                self.discovered_nodes.push((peer.id().clone(), addr.clone(), new_expiration));
                            }

                            discovered.push((peer.id().clone(), addr));
                        }
                    }

                    break discovered;
                },
                MdnsPacket::ServiceDiscovery(disc) => {
                    disc.respond(Duration::from_secs(5 * 60));
                },
            }
        };

        // As the final step, we need to refresh `closest_expiration`.
        self.closest_expiration = self.discovered_nodes.iter()
            .fold(None, |exp, &(_, _, elem_exp)| {
                Some(exp.map(|exp| cmp::min(exp, elem_exp)).unwrap_or(elem_exp))
            })
            .map(Delay::new);
        Async::Ready(NetworkBehaviourAction::GenerateEvent(MdnsEvent::Discovered(DiscoveredAddrsIter {
            inner: discovered.into_iter(),
        })))
    }
}

impl<TSubstream> fmt::Debug for Mdns<TSubstream> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("Mdns")
            .field("service", &self.service)
            .finish()
    }
}

