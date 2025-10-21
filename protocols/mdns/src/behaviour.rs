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

use std::{
    cmp,
    collections::{
        hash_map::{Entry, HashMap},
        VecDeque,
    },
    convert::Infallible,
    fmt,
    future::Future,
    io, mem,
    net::IpAddr,
    pin::Pin,
    task::{Context, Poll, Waker},
    time::Instant,
};

use futures::{channel::mpsc, Stream, StreamExt};
use if_watch::IfEvent;
use iface::ListenAddressUpdate;
use libp2p_core::{multiaddr::Protocol, transport::PortUse, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    behaviour::FromSwarm, dummy, ConnectionDenied, ConnectionId, ListenAddresses, NetworkBehaviour,
    THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use smallvec::SmallVec;

use self::iface::InterfaceState;
use crate::{
    behaviour::{socket::AsyncSocket, timer::Builder},
    Config,
};

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

    #[track_caller]
    fn spawn(task: impl Future<Output = ()> + Send + 'static);
}

/// The type of a [`Behaviour`] using the `tokio` implementation.
#[cfg(feature = "tokio")]
pub mod tokio {
    use std::future::Future;

    use if_watch::tokio::IfWatcher;

    use super::Provider;
    use crate::behaviour::{socket::tokio::TokioUdpSocket, timer::tokio::TokioTimer};

    #[doc(hidden)]
    pub enum Tokio {}

    impl Provider for Tokio {
        type Socket = TokioUdpSocket;
        type Timer = TokioTimer;
        type Watcher = IfWatcher;

        fn new_watcher() -> Result<Self::Watcher, std::io::Error> {
            IfWatcher::new()
        }

        fn spawn(task: impl Future<Output = ()> + Send + 'static) {
            tokio::spawn(task);
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

    /// Channel for sending address updates to interface tasks.
    if_tasks: HashMap<IpAddr, mpsc::Sender<ListenAddressUpdate>>,

    query_response_receiver: mpsc::Receiver<(PeerId, Multiaddr, Instant)>,
    query_response_sender: mpsc::Sender<(PeerId, Multiaddr, Instant)>,

    /// List of nodes that we have discovered, the address, and when their TTL expires.
    ///
    /// Each combination of `PeerId` and `Multiaddr` can only appear once, but the same `PeerId`
    /// can appear multiple times.
    discovered_nodes: SmallVec<[(PeerId, Multiaddr, Instant); 8]>,

    /// Future that fires when the TTL of at least one node in `discovered_nodes` expires.
    ///
    /// `None` if `discovered_nodes` is empty.
    closest_expiration: Option<P::Timer>,

    /// The current set of listen addresses.
    listen_addresses: ListenAddresses,

    local_peer_id: PeerId,

    /// Pending behaviour events to be emitted.
    pending_events: VecDeque<ToSwarm<Event, Infallible>>,

    /// Pending address updates to send to interfaces.
    pending_address_updates: Vec<ListenAddressUpdate>,

    waker: Waker,
}

impl<P> Behaviour<P>
where
    P: Provider,
{
    /// Builds a new `Mdns` behaviour.
    pub fn new(config: Config, local_peer_id: PeerId) -> io::Result<Self> {
        let (tx, rx) = mpsc::channel(10); // Chosen arbitrarily.

        Ok(Self {
            config,
            if_watch: P::new_watcher()?,
            if_tasks: Default::default(),
            query_response_receiver: rx,
            query_response_sender: tx,
            discovered_nodes: Default::default(),
            closest_expiration: Default::default(),
            listen_addresses: Default::default(),
            local_peer_id,
            pending_events: Default::default(),
            pending_address_updates: Default::default(),
            waker: Waker::noop().clone(),
        })
    }

    /// Returns true if the given `PeerId` is in the list of nodes discovered through mDNS.
    #[deprecated(note = "Use `discovered_nodes` iterator instead.")]
    pub fn has_node(&self, peer_id: &PeerId) -> bool {
        self.discovered_nodes().any(|p| p == peer_id)
    }

    /// Returns the list of nodes that we have discovered through mDNS and that are not expired.
    pub fn discovered_nodes(&self) -> impl ExactSizeIterator<Item = &PeerId> {
        self.discovered_nodes.iter().map(|(p, _, _)| p)
    }

    /// Expires a node before the ttl.
    #[deprecated(note = "Unused API. Will be removed in the next release.")]
    pub fn expire_node(&mut self, peer_id: &PeerId) {
        let now = Instant::now();
        for (peer, _addr, expires) in &mut self.discovered_nodes {
            if peer == peer_id {
                *expires = now;
            }
        }
        self.closest_expiration = Some(P::Timer::at(now));
    }

    /// Try to send an address update to the interface task that matches the address' IP.
    ///
    /// Returns the address if the sending failed due to a full channel.
    fn try_send_address_update(
        &mut self,
        cx: &mut Context<'_>,
        update: ListenAddressUpdate,
    ) -> Option<ListenAddressUpdate> {
        let ip = update.ip_addr()?;
        let tx = self.if_tasks.get_mut(&ip)?;
        match tx.poll_ready(cx) {
            Poll::Ready(Ok(())) => {
                tx.start_send(update).expect("Channel is ready.");
                None
            }
            Poll::Ready(Err(e)) if e.is_disconnected() => {
                tracing::error!("`InterfaceState` for ip {ip} dropped");
                self.if_tasks.remove(&ip);
                None
            }
            _ => Some(update),
        }
    }
}

impl<P> NetworkBehaviour for Behaviour<P>
where
    P: Provider,
{
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        let Some(peer_id) = maybe_peer else {
            return Ok(vec![]);
        };

        Ok(self
            .discovered_nodes
            .iter()
            .filter(|(peer, _, _)| peer == &peer_id)
            .map(|(_, addr, _)| addr.clone())
            .collect())
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn on_connection_handler_event(
        &mut self,
        _: PeerId,
        _: ConnectionId,
        ev: THandlerOutEvent<Self>,
    ) {
        libp2p_core::util::unreachable(ev)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        if !self.listen_addresses.on_swarm_event(&event) {
            return;
        }
        if let Some(update) = ListenAddressUpdate::from_swarm(event).and_then(|update| {
            self.try_send_address_update(&mut Context::from_waker(&self.waker.clone()), update)
        }) {
            self.pending_address_updates.push(update);
        }
    }

    #[tracing::instrument(level = "trace", name = "NetworkBehaviour::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        loop {
            // Send address updates to interface tasks.
            for update in mem::take(&mut self.pending_address_updates) {
                if let Some(update) = self.try_send_address_update(cx, update) {
                    self.pending_address_updates.push(update);
                }
            }

            // Check for pending events and emit them.
            if let Some(event) = self.pending_events.pop_front() {
                return Poll::Ready(event);
            }

            // Poll ifwatch.
            while let Poll::Ready(Some(event)) = Pin::new(&mut self.if_watch).poll_next(cx) {
                match event {
                    Ok(IfEvent::Up(inet)) => {
                        let ip_addr = inet.addr();
                        if ip_addr.is_loopback() {
                            continue;
                        }
                        if ip_addr.is_ipv4() && self.config.enable_ipv6
                            || ip_addr.is_ipv6() && !self.config.enable_ipv6
                        {
                            continue;
                        }
                        if let Entry::Vacant(e) = self.if_tasks.entry(ip_addr) {
                            let (addr_tx, addr_rx) = mpsc::channel(10); // Chosen arbitrarily.
                            let listen_addresses = self
                                .listen_addresses
                                .iter()
                                .filter(|multiaddr| multiaddr_matches_ip(multiaddr, &ip_addr))
                                .cloned()
                                .collect();
                            match InterfaceState::<P::Socket, P::Timer>::new(
                                ip_addr,
                                self.config.clone(),
                                self.local_peer_id,
                                listen_addresses,
                                addr_rx,
                                self.query_response_sender.clone(),
                            ) {
                                Ok(iface_state) => {
                                    P::spawn(iface_state);
                                    e.insert(addr_tx);
                                }
                                Err(err) => {
                                    tracing::error!("failed to create `InterfaceState`: {}", err)
                                }
                            }
                        }
                    }
                    Ok(IfEvent::Down(inet)) => {
                        if self.if_tasks.remove(&inet.addr()).is_some() {
                            tracing::info!(instance=%inet.addr(), "dropping instance");
                        }
                    }
                    Err(err) => tracing::error!("if watch returned an error: {}", err),
                }
            }
            // Emit discovered event.
            let mut discovered = Vec::new();

            while let Poll::Ready(Some((peer, addr, expiration))) =
                self.query_response_receiver.poll_next_unpin(cx)
            {
                if let Some((_, _, cur_expires)) = self
                    .discovered_nodes
                    .iter_mut()
                    .find(|(p, a, _)| *p == peer && *a == addr)
                {
                    *cur_expires = cmp::max(*cur_expires, expiration);
                } else {
                    tracing::info!(%peer, address=%addr, "discovered peer on address");
                    self.discovered_nodes.push((peer, addr.clone(), expiration));
                    discovered.push((peer, addr.clone()));

                    self.pending_events
                        .push_back(ToSwarm::NewExternalAddrOfPeer {
                            peer_id: peer,
                            address: addr,
                        });
                }
            }

            if !discovered.is_empty() {
                let event = Event::Discovered(discovered);
                // Push to the front of the queue so that the behavior event is reported before
                // the individual discovered addresses.
                self.pending_events
                    .push_front(ToSwarm::GenerateEvent(event));
                continue;
            }
            // Emit expired event.
            let now = Instant::now();
            let mut closest_expiration = None;
            let mut expired = Vec::new();
            self.discovered_nodes.retain(|(peer, addr, expiration)| {
                if *expiration <= now {
                    tracing::info!(%peer, address=%addr, "expired peer on address");
                    expired.push((*peer, addr.clone()));
                    return false;
                }
                closest_expiration =
                    Some(closest_expiration.unwrap_or(*expiration).min(*expiration));
                true
            });
            if !expired.is_empty() {
                let event = Event::Expired(expired);
                self.pending_events.push_back(ToSwarm::GenerateEvent(event));
                continue;
            }
            if let Some(closest_expiration) = closest_expiration {
                let mut timer = P::Timer::at(closest_expiration);
                let _ = Pin::new(&mut timer).poll_next(cx);

                self.closest_expiration = Some(timer);
            }

            self.waker = cx.waker().clone();
            return Poll::Pending;
        }
    }
}

fn multiaddr_matches_ip(addr: &Multiaddr, ip: &IpAddr) -> bool {
    match addr.iter().next() {
        Some(Protocol::Ip4(ipv4)) => &IpAddr::V4(ipv4) == ip,
        Some(Protocol::Ip6(ipv6)) => &IpAddr::V6(ipv6) == ip,
        _ => false,
    }
}

/// Event that can be produced by the `Mdns` behaviour.
#[derive(Debug, Clone)]
pub enum Event {
    /// Discovered nodes through mDNS.
    Discovered(Vec<(PeerId, Multiaddr)>),

    /// The given combinations of `PeerId` and `Multiaddr` have expired.
    ///
    /// Each discovered record has a time-to-live. When this TTL expires and the address hasn't
    /// been refreshed, we remove it from the list and emit it as an `Expired` event.
    Expired(Vec<(PeerId, Multiaddr)>),
}
