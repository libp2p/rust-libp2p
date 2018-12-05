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

use crate::dial_handler::PeriodicPingHandler;
use futures::prelude::*;
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{protocols_handler::ProtocolsHandler, PeerId};
use std::marker::PhantomData;
use tokio_io::{AsyncRead, AsyncWrite};
use void::Void;

/// Network behaviour that handles receiving pings sent by other nodes.
pub struct PeriodicPing<TSubstream> {
    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> PeriodicPing<TSubstream> {
    /// Creates a `PeriodicPing`.
    pub fn new() -> Self {
        PeriodicPing {
            marker: PhantomData,
        }
    }
}

impl<TSubstream> Default for PeriodicPing<TSubstream> {
    #[inline]
    fn default() -> Self {
        PeriodicPing::new()
    }
}

impl<TSubstream, TTopology> NetworkBehaviour<TTopology> for PeriodicPing<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = PeriodicPingHandler<TSubstream>;
    type OutEvent = Void;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        PeriodicPingHandler::new()
    }

    fn inject_connected(&mut self, _: PeerId, _: ConnectedPoint) {}

    fn inject_disconnected(&mut self, _: &PeerId, _: ConnectedPoint) {}

    fn inject_node_event(
        &mut self,
        _: PeerId,
        _: <Self::ProtocolsHandler as ProtocolsHandler>::OutEvent,
    ) {
    }

    fn poll(
        &mut self,
        _: &mut PollParameters<TTopology>,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        Async::NotReady
    }
}
