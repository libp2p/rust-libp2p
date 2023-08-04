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

use libp2p_core::{Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    dummy, ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, PollParameters, THandler,
    THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use void::Void;

use std::{
    fmt,
    task::{Context, Poll},
};

/// A [`NetworkBehaviour`] that enforces a set of memory usage based limits.
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
///   limits: memory_connection_limits::Behaviour
/// }
/// ```
pub struct Behaviour {
    max_process_memory_usage_bytes: Option<usize>,
    max_process_memory_usage_percentage: Option<f64>,
    system_physical_memory_bytes: usize,
}

impl Behaviour {
    /// Sets the process memory usage threshold in absolute bytes.
    ///
    /// New inbound and outbound connections will be denied when the threshold is reached.
    pub fn with_max_bytes(bytes: usize) -> Self {
        let mut b = Self::new();
        b.update_max_bytes(bytes);
        b
    }

    /// Sets the process memory usage threshold in the percentage of the total physical memory,
    /// all pending connections will be dropped when the threshold is exeeded
    pub fn with_max_percentage(percentage: f64) -> Self {
        let mut b = Self::new();
        b.update_max_percentage(percentage);
        b
    }

    /// Updates the process memory usage threshold in bytes,
    pub fn update_max_bytes(&mut self, bytes: usize) {
        self.max_process_memory_usage_bytes = Some(bytes);
    }

    /// Updates the process memory usage threshold in the percentage of the total physical memory,
    pub fn update_max_percentage(&mut self, percentage: f64) {
        self.max_process_memory_usage_percentage = Some(percentage);
    }

    fn new() -> Self {
        use sysinfo::{RefreshKind, SystemExt};

        let system_info = sysinfo::System::new_with_specifics(RefreshKind::new().with_memory());

        Self {
            max_process_memory_usage_bytes: None,
            max_process_memory_usage_percentage: None,
            system_physical_memory_bytes: system_info.total_memory() as usize,
        }
    }

    fn check_limit(&self) -> Result<(), ConnectionDenied> {
        if let Some(max_allowed_bytes) = self.max_allowed_bytes() {
            if let Some(stats) = memory_stats::memory_stats() {
                if stats.physical_mem > max_allowed_bytes {
                    return Err(ConnectionDenied::new(MemoryUsageLimitExceeded {
                        process_physical_memory_bytes: stats.physical_mem,
                        max_allowed_bytes,
                    }));
                }
            } else {
                log::warn!("Failed to retrive process memory stats");
            }
        }

        Ok(())
    }

    fn max_allowed_bytes(&self) -> Option<usize> {
        let max_process_memory_usage_percentage = self
            .max_process_memory_usage_percentage
            .map(|p| (self.system_physical_memory_bytes as f64 * p).round() as usize);
        match (
            self.max_process_memory_usage_bytes,
            max_process_memory_usage_percentage,
        ) {
            (None, None) => None,
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
        }
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
        self.check_limit()
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
