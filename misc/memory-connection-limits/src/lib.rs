// Copyright 2023 Protocol Labs.
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

mod connection_limits;
pub use connection_limits::{MemoryUsageBasedConnectionLimits, MemoryUsageLimitExceeded};

use libp2p_core::{Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    dummy, ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, PollParameters, THandler,
    THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use void::Void;

use std::{
    task::{Context, Poll},
    time::{Duration, Instant},
};

/// A [`NetworkBehaviour`] that enforces a set of memory usage based [`ConnectionLimits`].
///
/// For these limits to take effect, this needs to be composed into the behaviour tree of your application.
///
/// If a connection is denied due to a limit, either a [`SwarmEvent::IncomingConnectionError`](libp2p_swarm::SwarmEvent::IncomingConnectionError)
/// or [`SwarmEvent::OutgoingConnectionError`](libp2p_swarm::SwarmEvent::OutgoingConnectionError) will be emitted.
/// The [`ListenError::Denied`](libp2p_swarm::ListenError::Denied) and respectively the [`DialError::Denied`](libp2p_swarm::DialError::Denied) variant
/// contain a [`ConnectionDenied`](libp2p_swarm::ConnectionDenied) type that can be downcast to [`MemoryUsageLimitExceeded`] error if (and only if) **this**
/// behaviour denied the connection.
///
/// If you employ multiple [`NetworkBehaviour`]s that manage connections, it may also be a different error.
///
/// # Example
///
/// ```rust
/// # use libp2p_identify as identify;
/// # use libp2p_swarm_derive::NetworkBehaviour;
/// # use libp2p_memory_connection_limits as memory_connection_limits;
///
/// #[derive(NetworkBehaviour)]
/// # #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
/// struct MyBehaviour {
///   identify: identify::Behaviour,
///   limits: connection_limits::Behaviour
/// }
/// ```
pub struct Behaviour {
    limits: MemoryUsageBasedConnectionLimits,
    mem_tracker: ProcessMemoryUsageTracker,
}

impl Behaviour {
    pub fn new(limits: MemoryUsageBasedConnectionLimits) -> Self {
        Self {
            limits,
            mem_tracker: ProcessMemoryUsageTracker::new(),
        }
    }

    pub fn update_limits(&mut self, limits: MemoryUsageBasedConnectionLimits) {
        self.limits = limits;
    }

    /// Sets memory usage refresh interval, default is 1s. Use `None` to always refresh
    pub fn with_memory_usage_refresh_interval(mut self, interval: Option<Duration>) -> Self {
        self.mem_tracker.refresh_interval = interval;
        self
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Void;

    fn handle_pending_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.limits.check_limit(&mut self.mem_tracker)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.limits.check_limit(&mut self.mem_tracker)?;
        Ok(vec![])
    }

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn on_swarm_event(&mut self, _: FromSwarm<Self::ConnectionHandler>) {}

    fn on_connection_handler_event(
        &mut self,
        _id: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        void::unreachable(event)
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ConnectionKind {
    PendingIncoming,
    PendingOutgoing,
    EstablishedIncoming,
    EstablishedOutgoing,
    EstablishedPerPeer,
    EstablishedTotal,
}

#[derive(Debug)]
struct ProcessMemoryUsageTracker {
    refresh_interval: Option<Duration>,
    last_checked: Instant,
    physical_bytes: usize,
    virtual_bytes: usize,
}

impl ProcessMemoryUsageTracker {
    fn new() -> Self {
        const DEFAULT_MEMORY_USAGE_REFRESH_INTERVAL: Duration = Duration::from_millis(1000);

        let stats = memory_stats::memory_stats();
        if stats.is_none() {
            log::warn!("Failed to retrive process memory stats");
        }
        Self {
            refresh_interval: Some(DEFAULT_MEMORY_USAGE_REFRESH_INTERVAL),
            last_checked: Instant::now(),
            physical_bytes: stats.map(|s| s.physical_mem).unwrap_or_default(),
            virtual_bytes: stats
                .map(|s: memory_stats::MemoryStats| s.virtual_mem)
                .unwrap_or_default(),
        }
    }

    fn need_refresh(&self) -> bool {
        if let Some(refresh_interval) = self.refresh_interval {
            self.last_checked + refresh_interval < Instant::now()
        } else {
            true
        }
    }

    fn refresh(&mut self) {
        if let Some(stats) = memory_stats::memory_stats() {
            self.physical_bytes = stats.physical_mem;
            self.virtual_bytes = stats.virtual_mem;
        } else {
            log::warn!("Failed to retrive process memory stats");
        }
        self.last_checked = Instant::now();
    }

    fn refresh_if_needed(&mut self) {
        if self.need_refresh() {
            self.refresh();
        }
    }
}
