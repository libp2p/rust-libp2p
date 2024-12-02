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
    collections::hash_map::{Entry, HashMap},
    fmt,
    future::Future,
    io,
    net::IpAddr,
    pin::Pin,
    sync::{Arc, RwLock},
    task::{Context, Poll},
    time::Instant,
};

use futures::{channel::mpsc, Stream, StreamExt};
use if_watch::IfEvent;
use libp2p_core::{transport::PortUse, Endpoint, Multiaddr};
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

    type TaskHandle: Abort;

    /// Create a new instance of the `IfWatcher` type.
    fn new_watcher() -> Result<Self::Watcher, std::io::Error>;

    #[track_caller]
    fn spawn(task: impl Future<Output = ()> + Send + 'static) -> Self::TaskHandle;
}

#[allow(unreachable_pub)] // Not re-exported.
pub trait Abort {
    fn abort(self);
}

/// The type of a [`Behaviour`] using the `async-io` implementation.
#[cfg(feature = "async-io")]
pub mod async_io {
    use std::future::Future;

    use async_std::task::JoinHandle;
    use if_watch::smol::IfWatcher;

    use super::Provider;
    use crate::behaviour::{socket::asio::AsyncUdpSocket, timer::asio::AsyncTimer, Abort};

    #[doc(hidden)]
    pub enum AsyncIo {}

    impl Provider for AsyncIo {
        type Socket = AsyncUdpSocket;
        type Timer = AsyncTimer;
        type Watcher = IfWatcher;
        type TaskHandle = JoinHandle<()>;

        fn new_watcher() -> Result<Self::Watcher, std::io::Error> {
            IfWatcher::new()
        }

        fn spawn(task: impl Future<Output = ()> + Send + 'static) -> JoinHandle<()> {
            async_std::task::spawn(task)
        }
    }

    impl Abort for JoinHandle<()> {
        fn abort(self) {
            async_std::task::spawn(self.cancel());
        }
    }

    pub type Behaviour = super::Behaviour<AsyncIo>;
}

/// The type of a [`Behaviour`] using the `tokio` implementation.
#[cfg(feature = "tokio")]
pub mod tokio {
    use std::future::Future;

    use if_watch::tokio::IfWatcher;
    use tokio::task::JoinHandle;

    use super::Provider;
    use crate::behaviour::{socket::tokio::TokioUdpSocket, timer::tokio::TokioTimer, Abort};

    #[doc(hidden)]
    pub enum Tokio {}

    impl Provider for Tokio {
        type Socket = TokioUdpSocket;
        type Timer = TokioTimer;
        type Watcher = IfWatcher;
        type TaskHandle = JoinHandle<()>;

        fn new_watcher() -> Result<Self::Watcher, std::io::Error> {
            IfWatcher::new()
        }

        fn spawn(task: impl Future<Output = ()> + Send + 'static) -> Self::TaskHandle {
            tokio::spawn(task)
        }
    }

    impl Abort for JoinHandle<()> {
        fn abort(self) {
            JoinHandle::abort(&self)
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

    /// Handles to tasks running the mDNS queries.
    if_tasks: HashMap<IpAddr, P::TaskHandle>,

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
    ///
    /// This is shared across all interface tasks using an [`RwLock`].
    /// The [`Behaviour`] updates this upon new [`FromSwarm`]
    /// events where as [`InterfaceState`]s read from it to answer inbound mDNS queries.
    listen_addresses: Arc<RwLock<ListenAddresses>>,

    local_peer_id: PeerId,
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
        let peer_id = match maybe_peer {
            None => return Ok(vec![]),
            Some(peer) => peer,
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
        self.listen_addresses
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .on_swarm_event(&event);
    }

    #[tracing::instrument(level = "trace", name = "NetworkBehaviour::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
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
                    if let Entry::Vacant(e) = self.if_tasks.entry(addr) {
                        match InterfaceState::<P::Socket, P::Timer>::new(
                            addr,
                            self.config.clone(),
                            self.local_peer_id,
                            self.listen_addresses.clone(),
                            self.query_response_sender.clone(),
                        ) {
                            Ok(iface_state) => {
                                e.insert(P::spawn(iface_state));
                            }
                            Err(err) => {
                                tracing::error!("failed to create `InterfaceState`: {}", err)
                            }
                        }
                    }
                }
                Ok(IfEvent::Down(inet)) => {
                    if let Some(handle) = self.if_tasks.remove(&inet.addr()) {
                        tracing::info!(instance=%inet.addr(), "dropping instance");

                        handle.abort();
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
                discovered.push((peer, addr));
            }
        }

        if !discovered.is_empty() {
            let event = Event::Discovered(discovered);
            return Poll::Ready(ToSwarm::GenerateEvent(event));
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
            closest_expiration = Some(closest_expiration.unwrap_or(*expiration).min(*expiration));
            true
        });
        if !expired.is_empty() {
            let event = Event::Expired(expired);
            return Poll::Ready(ToSwarm::GenerateEvent(event));
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
