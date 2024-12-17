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

use std::{
    convert::Infallible,
    fmt,
    task::{Context, Poll},
    time::{Duration, Instant},
};

use libp2p_core::{transport::PortUse, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    dummy, ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use sysinfo::MemoryRefreshKind;

/// A [`NetworkBehaviour`] that enforces a set of memory usage based limits.
///
/// For these limits to take effect, this needs to be composed
/// into the behaviour tree of your application.
///
/// If a connection is denied due to a limit, either a
/// [`SwarmEvent::IncomingConnectionError`](libp2p_swarm::SwarmEvent::IncomingConnectionError)
/// or [`SwarmEvent::OutgoingConnectionError`](libp2p_swarm::SwarmEvent::OutgoingConnectionError)
/// will be emitted. The [`ListenError::Denied`](libp2p_swarm::ListenError::Denied) and respectively
/// the [`DialError::Denied`](libp2p_swarm::DialError::Denied) variant
/// contain a [`ConnectionDenied`] type that can be downcast to [`MemoryUsageLimitExceeded`] error
/// if (and only if) **this** behaviour denied the connection.
///
/// If you employ multiple [`NetworkBehaviour`]s that manage connections,
/// it may also be a different error.
///
/// [Behaviour::with_max_bytes] and [Behaviour::with_max_percentage] are mutually exclusive.
/// If you need to employ both of them,
/// compose two instances of [Behaviour] into your custom behaviour.
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
///     identify: identify::Behaviour,
///     limits: memory_connection_limits::Behaviour,
/// }
/// ```
pub struct Behaviour {
    max_allowed_bytes: usize,
    process_physical_memory_bytes: usize,
    last_refreshed: Instant,
}

/// The maximum duration for which the retrieved memory-stats
/// of the process are allowed to be stale.
///
/// Once exceeded, we will retrieve new stats.
const MAX_STALE_DURATION: Duration = Duration::from_millis(100);

impl Behaviour {
    /// Sets the process memory usage threshold in absolute bytes.
    ///
    /// New inbound and outbound connections will be denied when the threshold is reached.
    pub fn with_max_bytes(max_allowed_bytes: usize) -> Self {
        Self {
            max_allowed_bytes,
            process_physical_memory_bytes: memory_stats::memory_stats()
                .map(|s| s.physical_mem)
                .unwrap_or_default(),
            last_refreshed: Instant::now(),
        }
    }

    /// Sets the process memory usage threshold in the percentage of the total physical memory.
    ///
    /// New inbound and outbound connections will be denied when the threshold is reached.
    pub fn with_max_percentage(percentage: f64) -> Self {
        use sysinfo::{RefreshKind, System};

        let system_memory_bytes = System::new_with_specifics(
            RefreshKind::default().with_memory(MemoryRefreshKind::default().with_ram()),
        )
        .total_memory();

        Self::with_max_bytes((system_memory_bytes as f64 * percentage).round() as usize)
    }

    /// Gets the process memory usage threshold in bytes.
    pub fn max_allowed_bytes(&self) -> usize {
        self.max_allowed_bytes
    }

    fn check_limit(&mut self) -> Result<(), ConnectionDenied> {
        self.refresh_memory_stats_if_needed();

        if self.process_physical_memory_bytes > self.max_allowed_bytes {
            return Err(ConnectionDenied::new(MemoryUsageLimitExceeded {
                process_physical_memory_bytes: self.process_physical_memory_bytes,
                max_allowed_bytes: self.max_allowed_bytes,
            }));
        }

        Ok(())
    }

    fn refresh_memory_stats_if_needed(&mut self) {
        let now = Instant::now();

        if self.last_refreshed + MAX_STALE_DURATION > now {
            // Memory stats are reasonably recent, don't refresh.
            return;
        }

        let Some(stats) = memory_stats::memory_stats() else {
            tracing::warn!("Failed to retrieve process memory stats");
            return;
        };

        self.last_refreshed = now;
        self.process_physical_memory_bytes = stats.physical_mem;
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Infallible;

    fn handle_pending_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.check_limit()
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

    fn handle_pending_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.check_limit()?;
        Ok(vec![])
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

    fn on_swarm_event(&mut self, _: FromSwarm) {}

    fn on_connection_handler_event(
        &mut self,
        _id: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        // TODO: remove when Rust 1.82 is MSRV
        #[allow(unreachable_patterns)]
        libp2p_core::util::unreachable(event)
    }

    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        Poll::Pending
    }
}

/// A connection limit has been exceeded.
#[derive(Debug, Clone, Copy)]
pub struct MemoryUsageLimitExceeded {
    process_physical_memory_bytes: usize,
    max_allowed_bytes: usize,
}

impl MemoryUsageLimitExceeded {
    pub fn process_physical_memory_bytes(&self) -> usize {
        self.process_physical_memory_bytes
    }

    pub fn max_allowed_bytes(&self) -> usize {
        self.max_allowed_bytes
    }
}

impl std::error::Error for MemoryUsageLimitExceeded {}

impl fmt::Display for MemoryUsageLimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "process physical memory usage limit exceeded: process memory: {} bytes, max allowed: {} bytes",
            self.process_physical_memory_bytes,
            self.max_allowed_bytes,
        )
    }
}
