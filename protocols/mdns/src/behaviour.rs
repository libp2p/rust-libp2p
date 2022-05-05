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

mod iface;

use self::iface::InterfaceState;
use crate::MdnsConfig;
use async_io::Timer;
use futures::prelude::*;
use if_watch::{IfEvent, IfWatcher};
use libp2p_core::connection::ListenerId;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::{
    handler::DummyConnectionHandler, ConnectionHandler, NetworkBehaviour, NetworkBehaviourAction,
    PollParameters,
};
use smallvec::SmallVec;
use std::collections::hash_map::{Entry, HashMap};
use std::{cmp, fmt, io, net::IpAddr, pin::Pin, task::Context, task::Poll, time::Instant};

/// A `NetworkBehaviour` for mDNS. Automatically discovers peers on the local network and adds
/// them to the topology.
#[derive(Debug)]
pub struct Mdns {
    /// InterfaceState config.
    config: MdnsConfig,

    /// Iface watcher.
    if_watch: IfWatcher,

    /// Mdns interface states.
    iface_states: HashMap<IpAddr, InterfaceState>,

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

impl Mdns {
    /// Builds a new `Mdns` behaviour.
    pub async fn new(config: MdnsConfig) -> io::Result<Self> {
        let if_watch = if_watch::IfWatcher::new().await?;
        Ok(Self {
            config,
            if_watch,
            iface_states: Default::default(),
            discovered_nodes: Default::default(),
            closest_expiration: Default::default(),
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

    /// Expires a node before the ttl.
    pub fn expire_node(&mut self, peer_id: &PeerId) {
        let now = Instant::now();
        for (peer, _addr, expires) in &mut self.discovered_nodes {
            if peer == peer_id {
                *expires = now;
            }
        }
        self.closest_expiration = Some(Timer::at(now));
    }
}

impl NetworkBehaviour for Mdns {
    type ConnectionHandler = DummyConnectionHandler;
    type OutEvent = MdnsEvent;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        DummyConnectionHandler::default()
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        self.discovered_nodes
            .iter()
            .filter(|(peer, _, _)| peer == peer_id)
            .map(|(_, addr, _)| addr.clone())
            .collect()
    }

    fn inject_event(
        &mut self,
        _: PeerId,
        _: libp2p_core::connection::ConnectionId,
        ev: <Self::ConnectionHandler as ConnectionHandler>::OutEvent,
    ) {
        void::unreachable(ev)
    }

    fn inject_new_listen_addr(&mut self, _id: ListenerId, _addr: &Multiaddr) {
        log::trace!("waking interface state because listening address changed");
        for iface in self.iface_states.values_mut() {
            iface.fire_timer();
        }
    }

    fn inject_connection_closed(
        &mut self,
        peer: &PeerId,
        _: &libp2p_core::connection::ConnectionId,
        _: &libp2p_core::ConnectedPoint,
        _: Self::ConnectionHandler,
        remaining_established: usize,
    ) {
        if remaining_established == 0 {
            self.expire_node(peer);
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, DummyConnectionHandler>> {
        // Poll ifwatch.
        while let Poll::Ready(event) = Pin::new(&mut self.if_watch).poll(cx) {
            match event {
                Ok(IfEvent::Up(inet)) => {
                    let addr = inet.addr();
                    if addr.is_loopback() {
                        continue;
                    }
                    if addr.is_ipv4() && self.config.enable_ipv6
                        || addr.is_ipv6() && !self.config.enable_ipv6
                    {
                        continue;
                    }
                    if let Entry::Vacant(e) = self.iface_states.entry(addr) {
                        match InterfaceState::new(addr, self.config.clone()) {
                            Ok(iface_state) => {
                                e.insert(iface_state);
                            }
                            Err(err) => log::error!("failed to create `InterfaceState`: {}", err),
                        }
                    }
                }
                Ok(IfEvent::Down(inet)) => {
                    if self.iface_states.contains_key(&inet.addr()) {
                        log::info!("dropping instance {}", inet.addr());
                        self.iface_states.remove(&inet.addr());
                    }
                }
                Err(err) => log::error!("if watch returned an error: {}", err),
            }
        }
        // Emit discovered event.
        let mut discovered = SmallVec::<[(PeerId, Multiaddr); 4]>::new();
        for iface_state in self.iface_states.values_mut() {
            while let Some((peer, addr, expiration)) = iface_state.poll(cx, params) {
                if let Some((_, _, cur_expires)) = self
                    .discovered_nodes
                    .iter_mut()
                    .find(|(p, a, _)| *p == peer && *a == addr)
                {
                    *cur_expires = cmp::max(*cur_expires, expiration);
                } else {
                    log::info!("discovered: {} {}", peer, addr);
                    self.discovered_nodes.push((peer, addr.clone(), expiration));
                    discovered.push((peer, addr));
                }
            }
        }
        if !discovered.is_empty() {
            let event = MdnsEvent::Discovered(DiscoveredAddrsIter {
                inner: discovered.into_iter(),
            });
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }
        // Emit expired event.
        let now = Instant::now();
        let mut closest_expiration = None;
        let mut expired = SmallVec::<[(PeerId, Multiaddr); 4]>::new();
        self.discovered_nodes.retain(|(peer, addr, expiration)| {
            if *expiration <= now {
                log::info!("expired: {} {}", peer, addr);
                expired.push((*peer, addr.clone()));
                return false;
            }
            closest_expiration = Some(closest_expiration.unwrap_or(*expiration).min(*expiration));
            true
        });
        if !expired.is_empty() {
            let event = MdnsEvent::Expired(ExpiredAddrsIter {
                inner: expired.into_iter(),
            });
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }
        if let Some(closest_expiration) = closest_expiration {
            let mut timer = Timer::at(closest_expiration);
            let _ = Pin::new(&mut timer).poll(cx);
            self.closest_expiration = Some(timer);
        }
        Poll::Pending
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
    inner: smallvec::IntoIter<[(PeerId, Multiaddr); 4]>,
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

impl ExactSizeIterator for DiscoveredAddrsIter {}

impl fmt::Debug for DiscoveredAddrsIter {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("DiscoveredAddrsIter").finish()
    }
}

/// Iterator that produces the list of addresses that have expired.
pub struct ExpiredAddrsIter {
    inner: smallvec::IntoIter<[(PeerId, Multiaddr); 4]>,
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

impl ExactSizeIterator for ExpiredAddrsIter {}

impl fmt::Debug for ExpiredAddrsIter {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("ExpiredAddrsIter").finish()
    }
}
