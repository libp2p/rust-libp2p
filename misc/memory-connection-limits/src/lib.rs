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
    last_checked: Instant,
    physical_bytes: usize,
    virtual_bytes: usize,
}

impl ProcessMemoryUsageTracker {
    fn new() -> Self {
        let stats = memory_stats::memory_stats();
        if stats.is_none() {
            log::warn!("Failed to retrive process memory stats");
        }
        Self {
            last_checked: Instant::now(),
            physical_bytes: stats.map(|s| s.physical_mem).unwrap_or_default(),
            virtual_bytes: stats
                .map(|s: memory_stats::MemoryStats| s.virtual_mem)
                .unwrap_or_default(),
        }
    }

    fn need_refresh(&self) -> bool {
        const PROCESS_MEMORY_USAGE_CHECK_INTERVAL: Duration = Duration::from_millis(1000);

        self.last_checked + PROCESS_MEMORY_USAGE_CHECK_INTERVAL < Instant::now()
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

    fn total_bytes(&self) -> usize {
        self.physical_bytes + self.virtual_bytes
    }
}
