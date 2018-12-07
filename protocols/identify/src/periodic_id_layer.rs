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

use crate::periodic_id_handler::{PeriodicIdentification, PeriodicIdentificationEvent};
use crate::protocol::IdentifyInfo;
use futures::prelude::*;
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{protocols_handler::ProtocolsHandler, Multiaddr, PeerId};
use std::{collections::VecDeque, marker::PhantomData};
use tokio_io::{AsyncRead, AsyncWrite};
use void::Void;

/// Network behaviour that automatically identifies nodes periodically, and returns information
/// about them.
pub struct PeriodicIdentify<TSubstream> {
    /// Events that need to be produced outside when polling..
    events: VecDeque<NetworkBehaviourAction<Void, PeriodicIdentifyEvent>>,
    /// Marker to pin the generics.
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> PeriodicIdentify<TSubstream> {
    /// Creates a `PeriodicIdentify`.
    pub fn new() -> Self {
        PeriodicIdentify {
            events: VecDeque::new(),
            marker: PhantomData,
        }
    }
}

impl<TSubstream, TTopology> NetworkBehaviour<TTopology> for PeriodicIdentify<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = PeriodicIdentification<TSubstream>;
    type OutEvent = PeriodicIdentifyEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        PeriodicIdentification::new()
    }

    fn inject_connected(&mut self, _: PeerId, _: ConnectedPoint) {}

    fn inject_disconnected(&mut self, _: &PeerId, _: ConnectedPoint) {}

    fn inject_node_event(
        &mut self,
        peer_id: PeerId,
        event: <Self::ProtocolsHandler as ProtocolsHandler>::OutEvent,
    ) {
        match event {
            PeriodicIdentificationEvent::Identified(remote) => {
                self.events
                    .push_back(NetworkBehaviourAction::ReportObservedAddr {
                        address: remote.observed_addr.clone(),
                    });
                self.events
                    .push_back(NetworkBehaviourAction::GenerateEvent(PeriodicIdentifyEvent::Identified {
                        peer_id: peer_id,
                        info: remote.info,
                        observed_addr: remote.observed_addr,
                    }));
            }
            _ => (), // TODO: exhaustive pattern
        }
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
        if let Some(event) = self.events.pop_front() {
            return Async::Ready(event);
        }

        Async::NotReady
    }
}

/// Event generated by the `PeriodicIdentify`.
#[derive(Debug, Clone)]
pub enum PeriodicIdentifyEvent {
    /// We obtained identification information from the remote
    Identified {
        /// Peer that has been successfully identified.
        peer_id: PeerId,
        /// Information of the remote.
        info: IdentifyInfo,
        /// Address the remote observes us as.
        observed_addr: Multiaddr,
    },
}
