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
use futures::prelude::*;
use libp2p_core::protocols_handler::{ProtocolsHandler, ProtocolsHandlerSelect, ProtocolsHandlerUpgrErr};
use libp2p_core::swarm::{ConnectedPoint, NetworkBehaviour, NetworkBehaviourAction, PollParameters};
use libp2p_core::{Multiaddr, PeerId, PublicKey, either::EitherOutput};
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
    /// The public key of the local node. To report on the wire.
    local_public_key: PublicKey,
    /// For each peer we're connected to, the observed address to send back to it.
    observed_addresses: HashMap<PeerId, Multiaddr>,
    /// List of senders to answer, with the observed multiaddr.
    to_answer: SmallVec<[(PeerId, IdentifySender<TSubstream>, Multiaddr); 4]>,
    /// List of futures that send back information back to remotes.
    futures: SmallVec<[(PeerId, IdentifySenderFuture<TSubstream>); 4]>,
    /// Events that need to be produced outside when polling..
    events: VecDeque<NetworkBehaviourAction<EitherOutput<Void, Void>, IdentifyEvent>>,
}

impl<TSubstream> Identify<TSubstream> {
    /// Creates a `Identify`.
    pub fn new(protocol_version: String, agent_version: String, local_public_key: PublicKey) -> Self {
        Identify {
            protocol_version,
            agent_version,
            local_public_key,
            observed_addresses: HashMap::new(),
            to_answer: SmallVec::new(),
            futures: SmallVec::new(),
            events: VecDeque::new(),
        }
    }
}

impl<TSubstream> NetworkBehaviour for Identify<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = ProtocolsHandlerSelect<IdentifyListenHandler<TSubstream>, PeriodicIdHandler<TSubstream>>;
    type OutEvent = IdentifyEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        IdentifyListenHandler::new().select(PeriodicIdHandler::new())
    }

    fn addresses_of_peer(&mut self, _: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
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
                self.to_answer.push((peer_id, sender, observed.clone()));
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
        params: &mut PollParameters<'_>,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        if let Some(event) = self.events.pop_front() {
            return Async::Ready(event);
        }

        for (peer_id, sender, observed) in self.to_answer.drain() {
            // The protocol names can be bytes, but the identify protocol except UTF-8 strings.
            // There's not much we can do to solve this conflict except strip non-UTF-8 characters.
            let protocols = params
                .supported_protocols()
                .map(|p| String::from_utf8_lossy(p).to_string())
                .collect();

            let mut listen_addrs: Vec<_> = params.external_addresses().collect();
            listen_addrs.extend(params.listened_addresses().cloned());

            let send_back_info = IdentifyInfo {
                public_key: self.local_public_key.clone(),
                protocol_version: self.protocol_version.clone(),
                agent_version: self.agent_version.clone(),
                listen_addrs,
                protocols,
            };

            let future = sender.send(send_back_info, &observed);
            self.futures.push((peer_id, future));
        }

        // Removes each future one by one, and pushes them back if they're not ready.
        for n in (0..self.futures.len()).rev() {
            let (peer_id, mut future) = self.futures.swap_remove(n);
            match future.poll() {
                Ok(Async::Ready(())) => {
                    let event = IdentifyEvent::SendBack {
                        peer_id,
                        result: Ok(()),
                    };
                    return Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
                },
                Ok(Async::NotReady) => self.futures.push((peer_id, future)),
                Err(err) => {
                    let event = IdentifyEvent::SendBack {
                        peer_id,
                        result: Err(err),
                    };
                    return Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
                },
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
    /// Finished sending back our identification information to a remote.
    SendBack {
        /// Peer that we sent our identification info to.
        peer_id: PeerId,
        /// Contains the error that potentially happened when sending back.
        result: Result<(), io::Error>,
    },
}

#[cfg(test)]
mod tests {
    use crate::{Identify, IdentifyEvent};
    use futures::prelude::*;
    use libp2p_core::identity;
    use libp2p_core::{upgrade, upgrade::OutboundUpgradeExt, upgrade::InboundUpgradeExt, Swarm, Transport};
    use std::io;

    #[test]
    fn periodic_id_works() {
        let node1_key = identity::Keypair::generate_ed25519();
        let node1_public_key = node1_key.public();
        let node2_key = identity::Keypair::generate_ed25519();
        let node2_public_key = node2_key.public();

        let mut swarm1 = {
            // TODO: make creating the transport more elegant ; literaly half of the code of the test
            //       is about creating the transport
            let local_peer_id = node1_public_key.clone().into_peer_id();
            let transport = libp2p_tcp::TcpConfig::new()
                .with_upgrade(libp2p_secio::SecioConfig::new(node1_key))
                .and_then(move |out, endpoint| {
                    let peer_id = out.remote_key.into_peer_id();
                    let peer_id2 = peer_id.clone();
                    let upgrade = libp2p_mplex::MplexConfig::default()
                        .map_outbound(move |muxer| (peer_id, muxer))
                        .map_inbound(move |muxer| (peer_id2, muxer));
                    upgrade::apply(out.stream, upgrade, endpoint)
                })
                .map_err(|_| -> io::Error { panic!() });

            Swarm::new(transport, Identify::new("a".to_string(), "b".to_string(), node1_public_key.clone()), local_peer_id)
        };

        let mut swarm2 = {
            // TODO: make creating the transport more elegant ; literaly half of the code of the test
            //       is about creating the transport
            let local_peer_id = node2_public_key.clone().into();
            let transport = libp2p_tcp::TcpConfig::new()
                .with_upgrade(libp2p_secio::SecioConfig::new(node2_key))
                .and_then(move |out, endpoint| {
                    let peer_id = out.remote_key.into_peer_id();
                    let peer_id2 = peer_id.clone();
                    let upgrade = libp2p_mplex::MplexConfig::default()
                        .map_outbound(move |muxer| (peer_id, muxer))
                        .map_inbound(move |muxer| (peer_id2, muxer));
                    upgrade::apply(out.stream, upgrade, endpoint)
                })
                .map_err(|_| -> io::Error { panic!() });

            Swarm::new(transport, Identify::new("c".to_string(), "d".to_string(), node2_public_key.clone()), local_peer_id)
        };

        let actual_addr = Swarm::listen_on(&mut swarm1, "/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();
        Swarm::dial_addr(&mut swarm2, actual_addr).unwrap();

        let mut swarm1_good = false;
        let mut swarm2_good = false;

        tokio::runtime::current_thread::Runtime::new()
            .unwrap()
            .block_on(futures::future::poll_fn(move || -> Result<_, io::Error> {
                loop {
                    let mut swarm1_not_ready = false;
                    match swarm1.poll().unwrap() {
                        Async::Ready(Some(IdentifyEvent::Identified { info, .. })) => {
                            assert_eq!(info.public_key, node2_public_key);
                            assert_eq!(info.protocol_version, "c");
                            assert_eq!(info.agent_version, "d");
                            assert!(!info.protocols.is_empty());
                            assert!(info.listen_addrs.is_empty());
                            swarm1_good = true;
                        },
                        Async::Ready(Some(IdentifyEvent::SendBack { result: Ok(()), .. })) => (),
                        Async::Ready(_) => panic!(),
                        Async::NotReady => swarm1_not_ready = true,
                    }

                    match swarm2.poll().unwrap() {
                        Async::Ready(Some(IdentifyEvent::Identified { info, .. })) => {
                            assert_eq!(info.public_key, node1_public_key);
                            assert_eq!(info.protocol_version, "a");
                            assert_eq!(info.agent_version, "b");
                            assert!(!info.protocols.is_empty());
                            assert_eq!(info.listen_addrs.len(), 1);
                            swarm2_good = true;
                        },
                        Async::Ready(Some(IdentifyEvent::SendBack { result: Ok(()), .. })) => (),
                        Async::Ready(_) => panic!(),
                        Async::NotReady if swarm1_not_ready => break,
                        Async::NotReady => ()
                    }
                }

                if swarm1_good && swarm2_good {
                    Ok(Async::Ready(()))
                } else {
                    Ok(Async::NotReady)
                }
            }))
            .unwrap();
    }
}
