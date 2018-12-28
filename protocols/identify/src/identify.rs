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

use crate::listen_handler::IdentifyListenHandler;
use crate::periodic_id_handler::{PeriodicIdHandler, PeriodicIdHandlerEvent};
use crate::protocol::{IdentifyInfo, IdentifySender, IdentifySenderFuture};
use crate::topology::IdentifyTopology;
use futures::prelude::*;
use libp2p_core::protocols_handler::{ProtocolsHandler, ProtocolsHandlerSelect, ProtocolsHandlerUpgrErr};
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{Multiaddr, PeerId, either::EitherOutput};
use smallvec::SmallVec;
use std::{collections::HashMap, collections::VecDeque, io};
use tokio_io::{AsyncRead, AsyncWrite};
use void::Void;

/// Network behaviour that automatically identifies nodes periodically, returns information
/// about them, and answers identify queries from other nodes.
pub struct Identify<TSubstream> {
    /// Protocol version to send back to remotes.
    protocol_version: String,
    /// Agent version to send back to remotes.
    agent_version: String,
    /// For each peer we're connected to, the observed address to send back to it.
    observed_addresses: HashMap<PeerId, Multiaddr>,
    /// List of senders to answer, with the observed multiaddr.
    to_answer: SmallVec<[(IdentifySender<TSubstream>, Multiaddr); 4]>,
    /// List of futures that send back information back to remotes.
    futures: SmallVec<[IdentifySenderFuture<TSubstream>; 4]>,
    /// Events that need to be produced outside when polling..
    events: VecDeque<NetworkBehaviourAction<EitherOutput<Void, Void>, IdentifyEvent>>,
}

impl<TSubstream> Identify<TSubstream> {
    /// Creates a `Identify`.
    pub fn new(protocol_version: String, agent_version: String) -> Self {
        Identify {
            protocol_version,
            agent_version,
            observed_addresses: HashMap::new(),
            to_answer: SmallVec::new(),
            futures: SmallVec::new(),
            events: VecDeque::new(),
        }
    }
}

impl<TSubstream, TTopology> NetworkBehaviour<TTopology> for Identify<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
    TTopology: IdentifyTopology,
{
    type ProtocolsHandler = ProtocolsHandlerSelect<IdentifyListenHandler<TSubstream>, PeriodicIdHandler<TSubstream>>;
    type OutEvent = IdentifyEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        IdentifyListenHandler::new().select(PeriodicIdHandler::new())
    }

    fn inject_connected(&mut self, peer_id: PeerId, endpoint: ConnectedPoint) {
        let observed = match endpoint {
            ConnectedPoint::Dialer { address } => address,
            ConnectedPoint::Listener { send_back_addr, .. } => send_back_addr,
        };

        self.observed_addresses.insert(peer_id, observed);
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId, _: ConnectedPoint) {
        self.observed_addresses.remove(peer_id);
    }

    fn inject_node_event(
        &mut self,
        peer_id: PeerId,
        event: <Self::ProtocolsHandler as ProtocolsHandler>::OutEvent,
    ) {
        match event {
            EitherOutput::Second(PeriodicIdHandlerEvent::Identified(remote)) => {
                self.events
                    .push_back(NetworkBehaviourAction::GenerateEvent(IdentifyEvent::Identified {
                        peer_id,
                        info: remote.info,
                        observed_addr: remote.observed_addr.clone(),
                    }));
                self.events
                    .push_back(NetworkBehaviourAction::ReportObservedAddr {
                        address: remote.observed_addr,
                    });
            }
            EitherOutput::First(sender) => {
                let observed = self.observed_addresses.get(&peer_id)
                    .expect("We only receive events from nodes we're connected to. We insert \
                             into the hashmap when we connect to a node and remove only when we \
                             disconnect; QED");
                self.to_answer.push((sender, observed.clone()));
            }
            EitherOutput::Second(PeriodicIdHandlerEvent::IdentificationError(err)) => {
                self.events
                    .push_back(NetworkBehaviourAction::GenerateEvent(IdentifyEvent::Error {
                        peer_id,
                        error: err,
                    }));
            }
        }
    }

    fn poll(
        &mut self,
        params: &mut PollParameters<TTopology>,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        if let Some(event) = self.events.pop_front() {
            // We intercept identified events in order to insert the addresses in the topology.
            if let NetworkBehaviourAction::GenerateEvent(IdentifyEvent::Identified { ref peer_id, ref info, .. }) = event {
                let iter = info.listen_addrs.iter().cloned();
                params.topology().add_identify_discovered_addrs(peer_id, iter);
            }

            return Async::Ready(event);
        }

        for (sender, observed) in self.to_answer.drain() {
            // The protocol names can be bytes, but the identify protocol except UTF-8 strings.
            // There's not much we can do to solve this conflict except strip non-UTF-8 characters.
            let protocols = params
                .supported_protocols()
                .map(|p| String::from_utf8_lossy(p).to_string())
                .collect();

            let send_back_info = IdentifyInfo {
                public_key: params.local_public_key().clone(),
                protocol_version: self.protocol_version.clone(),
                agent_version: self.agent_version.clone(),
                listen_addrs: params.listened_addresses().cloned().collect(),
                protocols,
            };

            let future = sender.send(send_back_info, &observed);
            self.futures.push(future);
        }

        // Removes each future one by one, and pushes them back if they're not ready.
        for n in (0..self.futures.len()).rev() {
            let mut future = self.futures.swap_remove(n);
            match future.poll() {
                Ok(Async::Ready(())) => {}
                Ok(Async::NotReady) => self.futures.push(future),
                Err(_) => {},
            }
        }

        Async::NotReady
    }
}

/// Event generated by the `Identify`.
#[derive(Debug)]
pub enum IdentifyEvent {
    /// We obtained identification information from the remote
    Identified {
        /// Peer that has been successfully identified.
        peer_id: PeerId,
        /// Information of the remote.
        info: IdentifyInfo,
        /// Address the remote observes us as.
        observed_addr: Multiaddr,
    },
    /// Error while attempting to identify the remote.
    Error {
        /// Peer that we fail to identify.
        peer_id: PeerId,
        /// The error that happened.
        error: ProtocolsHandlerUpgrErr<io::Error>,
    },
}
