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
    task::{Context, Poll},
};

use libp2p_core::{transport::PortUse, Endpoint, Multiaddr};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    dummy, ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};

#[derive(libp2p_swarm_derive::NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
pub(crate) struct TestBehaviour {
    pub(crate) connection_limits: libp2p_memory_connection_limits::Behaviour,
    pub(crate) mem_consumer: ConsumeMemoryBehaviour1MBPending0Established,
}

pub(crate) type ConsumeMemoryBehaviour1MBPending0Established =
    ConsumeMemoryBehaviour<{ 1024 * 1024 }, 0>;

#[derive(Default)]
pub(crate) struct ConsumeMemoryBehaviour<const MEM_PENDING: usize, const MEM_ESTABLISHED: usize> {
    mem_pending: Vec<Vec<u8>>,
    mem_established: Vec<Vec<u8>>,
}

impl<const MEM_PENDING: usize, const MEM_ESTABLISHED: usize>
    ConsumeMemoryBehaviour<MEM_PENDING, MEM_ESTABLISHED>
{
    fn handle_pending(&mut self) {
        // 1MB
        self.mem_pending.push(vec![1; MEM_PENDING]);
    }

    fn handle_established(&mut self) {
        // 1MB
        self.mem_established.push(vec![1; MEM_ESTABLISHED]);
    }
}

impl<const MEM_PENDING: usize, const MEM_ESTABLISHED: usize> NetworkBehaviour
    for ConsumeMemoryBehaviour<MEM_PENDING, MEM_ESTABLISHED>
{
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Infallible;

    fn handle_pending_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.handle_pending();
        Ok(())
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.handle_pending();
        Ok(vec![])
    }

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.handle_established();
        Ok(dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.handle_established();
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
