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
mod socket;
mod timer;

use self::iface::InterfaceState;
use crate::behaviour::{socket::AsyncSocket, timer::Builder};
use crate::Config;
use futures::Stream;
use if_watch::IfEvent;
use libp2p_core::{Multiaddr, PeerId};
use libp2p_swarm::behaviour::FromSwarm;
use libp2p_swarm::{
    dummy, ListenAddresses, NetworkBehaviour, NetworkBehaviourAction, PollParameters,
    THandlerOutEvent,
};
use smallvec::SmallVec;
use std::collections::hash_map::{Entry, HashMap};
use std::{cmp, fmt, io, net::IpAddr, pin::Pin, task::Context, task::Poll, time::Instant};

/// An abstraction to allow for compatibility with various async runtimes.
pub trait Provider: 'static {
    /// The Async Socket type.
    type Socket: AsyncSocket;
    /// The Async Timer type.
    type Timer: Builder + Stream;
    /// The IfWatcher type.
    type Watcher: Stream<Item = std::io::Result<IfEvent>> + fmt::Debug + Unpin;

    /// Create a new instance of the `IfWatcher` type.
    fn new_watcher() -> Result<Self::Watcher, std::io::Error>;
}

/// The type of a [`Behaviour`] using the `async-io` implementation.
#[cfg(feature = "async-io")]
pub mod async_io {
    use super::Provider;
    use crate::behaviour::{socket::asio::AsyncUdpSocket, timer::asio::AsyncTimer};
    use if_watch::smol::IfWatcher;

    #[doc(hidden)]
    pub enum AsyncIo {}

    impl Provider for AsyncIo {
        type Socket = AsyncUdpSocket;
        type Timer = AsyncTimer;
        type Watcher = IfWatcher;

        fn new_watcher() -> Result<Self::Watcher, std::io::Error> {
            IfWatcher::new()
        }
    }

    pub type Behaviour = super::Behaviour<AsyncIo>;
}

/// The type of a [`Behaviour`] using the `tokio` implementation.
#[cfg(feature = "tokio")]
pub mod tokio {
    use super::Provider;
    use crate::behaviour::{socket::tokio::TokioUdpSocket, timer::tokio::TokioTimer};
    use if_watch::tokio::IfWatcher;

    #[doc(hidden)]
    pub enum Tokio {}

    impl Provider for Tokio {
        type Socket = TokioUdpSocket;
        type Timer = TokioTimer;
        type Watcher = IfWatcher;

        fn new_watcher() -> Result<Self::Watcher, std::io::Error> {
            IfWatcher::new()
        }
    }

    pub type Behaviour = super::Behaviour<Tokio>;
}

/// A `NetworkBehaviour` for mDNS. Automatically discovers peers on the local network and adds
/// them to the topology.
#[derive(Debug)]
pub struct Behaviour<P>
where
    P: Provider,
{
    /// InterfaceState config.
    config: Config,

    /// Iface watcher.
    if_watch: P::Watcher,

    /// Mdns interface states.
    iface_states: HashMap<IpAddr, InterfaceState<P::Socket, P::Timer>>,

    /// List of nodes that we have discovered, the address, and when their TTL expires.
    ///
    /// Each combination of `PeerId` and `Multiaddr` can only appear once, but the same `PeerId`
    /// can appear multiple times.
    discovered_nodes: SmallVec<[(PeerId, Multiaddr, Instant); 8]>,

    /// Future that fires when the TTL of at least one node in `discovered_nodes` expires.
    ///
    /// `None` if `discovered_nodes` is empty.
    closest_expiration: Option<P::Timer>,

    listen_addresses: ListenAddresses,

    local_peer_id: PeerId,
}

impl<P> Behaviour<P>
where
    P: Provider,
{
    /// Builds a new `Mdns` behaviour.
    pub fn new(config: Config, local_peer_id: PeerId) -> io::Result<Self> {
        Ok(Self {
            config,
            if_watch: P::new_watcher()?,
            iface_states: Default::default(),
            discovered_nodes: Default::default(),
            closest_expiration: Default::default(),
            listen_addresses: Default::default(),
            local_peer_id,
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
        self.closest_expiration = Some(P::Timer::at(now));
    }
}

impl<P> NetworkBehaviour for Behaviour<P>
where
    P: Provider,
{
    type ConnectionHandler = dummy::ConnectionHandler;
    type OutEvent = Event;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        dummy::ConnectionHandler
    }

    fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
        self.discovered_nodes
            .iter()
            .filter(|(peer, _, _)| peer == peer_id)
            .map(|(_, addr, _)| addr.clone())
            .collect()
    }

    fn on_connection_handler_event(
        &mut self,
        _: PeerId,
        _: libp2p_swarm::ConnectionId,
        ev: THandlerOutEvent<Self>,
    ) {
        void::unreachable(ev)
    }

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
        self.listen_addresses.on_swarm_event(&event);

        match event {
            FromSwarm::NewListener(_) => {
                log::trace!("waking interface state because listening address changed");
                for iface in self.iface_states.values_mut() {
                    iface.fire_timer();
                }
            }
            FromSwarm::ConnectionClosed(_)
            | FromSwarm::ConnectionEstablished(_)
            | FromSwarm::DialFailure(_)
            | FromSwarm::AddressChange(_)
            | FromSwarm::ListenFailure(_)
            | FromSwarm::NewListenAddr(_)
            | FromSwarm::ExpiredListenAddr(_)
            | FromSwarm::ListenerError(_)
            | FromSwarm::ListenerClosed(_)
            | FromSwarm::NewExternalAddr(_)
            | FromSwarm::ExpiredExternalAddr(_) => {}
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, dummy::ConnectionHandler>> {
        // Poll ifwatch.
        while let Poll::Ready(Some(event)) = Pin::new(&mut self.if_watch).poll_next(cx) {
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
                        match InterfaceState::new(addr, self.config.clone(), self.local_peer_id) {
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
            while let Poll::Ready((peer, addr, expiration)) =
                iface_state.poll(cx, &self.listen_addresses)
            {
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
            let event = Event::Discovered(DiscoveredAddrsIter {
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
            let event = Event::Expired(ExpiredAddrsIter {
                inner: expired.into_iter(),
            });
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
        }
        if let Some(closest_expiration) = closest_expiration {
            let mut timer = P::Timer::at(closest_expiration);
            let _ = Pin::new(&mut timer).poll_next(cx);

            self.closest_expiration = Some(timer);
        }
        Poll::Pending
    }
}

/// Event that can be produced by the `Mdns` behaviour.
#[derive(Debug)]
pub enum Event {
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
