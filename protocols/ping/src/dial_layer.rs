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

use futures::prelude::*;
use libp2p_core::nodes::{ConnectedPoint, NetworkBehavior, NetworkBehaviorAction};
use libp2p_core::{nodes::protocols_handler::ProtocolsHandler, PeerId};
use std::marker::PhantomData;
use tokio_io::{AsyncRead, AsyncWrite};
use void::Void;
use PeriodicPingHandler;

/// Network behaviour that handles receiving pings sent by other nodes.
pub struct PeriodicPingBehaviour<TSubstream> {
    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> PeriodicPingBehaviour<TSubstream> {
    /// Creates a `PeriodicPingBehaviour`.
    pub fn new() -> Self {
        PeriodicPingBehaviour {
            marker: PhantomData,
        }
    }
}

impl<TSubstream> Default for PeriodicPingBehaviour<TSubstream> {
    #[inline]
    fn default() -> Self {
        PeriodicPingBehaviour::new()
    }
}

impl<TSubstream> NetworkBehavior for PeriodicPingBehaviour<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite + Send + Sync + 'static,
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
    ) -> Async<
        NetworkBehaviorAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        Async::NotReady
    }
}
