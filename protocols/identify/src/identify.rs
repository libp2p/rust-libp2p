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

use crate::handler::{IdentifyHandler, IdentifyHandlerEvent};
use crate::protocol::{IdentifyInfo, ReplySubstream, ReplyFuture};
use futures::prelude::*;
use libp2p_core::{
    ConnectedPoint,
    Multiaddr,
    PeerId,
    PublicKey,
    upgrade::{Negotiated, UpgradeError}
};
use libp2p_swarm::{
    NetworkBehaviour,
    NetworkBehaviourAction,
    PollParameters,
    ProtocolsHandler,
    ProtocolsHandlerUpgrErr
};
use std::{collections::HashMap, collections::VecDeque, io};
use tokio_io::{AsyncRead, AsyncWrite};

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
    /// Pending replies to send.
    pending_replies: VecDeque<Reply<TSubstream>>,
    /// Pending events to be emitted when polled.
    events: VecDeque<NetworkBehaviourAction<(), IdentifyEvent>>,
}

/// A pending reply to an inbound identification request.
enum Reply<TSubstream> {
    /// The reply is queued for sending.
    Queued {
        peer: PeerId,
        io: ReplySubstream<Negotiated<TSubstream>>,
        observed: Multiaddr
    },
    /// The reply is being sent.
    Sending {
        peer: PeerId,
        io: ReplyFuture<Negotiated<TSubstream>>
    }
}

impl<TSubstream> Identify<TSubstream> {
    /// Creates a new `Identify` network behaviour.
    pub fn new(protocol_version: String, agent_version: String, local_public_key: PublicKey) -> Self {
        Identify {
            protocol_version,
            agent_version,
            local_public_key,
            observed_addresses: HashMap::new(),
            pending_replies: VecDeque::new(),
            events: VecDeque::new(),
        }
    }
}

impl<TSubstream> NetworkBehaviour for Identify<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type ProtocolsHandler = IdentifyHandler<TSubstream>;
    type OutEvent = IdentifyEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        IdentifyHandler::new()
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
            IdentifyHandlerEvent::Identified(remote) => {
                self.events.push_back(
                    NetworkBehaviourAction::GenerateEvent(
                        IdentifyEvent::Received {
                            peer_id,
                            info: remote.info,
                            observed_addr: remote.observed_addr.clone(),
                        }));
                self.events.push_back(
                    NetworkBehaviourAction::ReportObservedAddr {
                        address: remote.observed_addr,
                    });
            }
            IdentifyHandlerEvent::Identify(sender) => {
                let observed = self.observed_addresses.get(&peer_id)
                    .expect("We only receive events from nodes we're connected to. We insert \
                             into the hashmap when we connect to a node and remove only when we \
                             disconnect; QED");
                self.pending_replies.push_back(
                    Reply::Queued {
                        peer: peer_id,
                        io: sender,
                        observed: observed.clone()
                    });
            }
            IdentifyHandlerEvent::IdentificationError(error) => {
                self.events.push_back(
                    NetworkBehaviourAction::GenerateEvent(
                        IdentifyEvent::Error { peer_id, error }));
            }
        }
    }

    fn poll(
        &mut self,
        params: &mut impl PollParameters,
    ) -> Async<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        if let Some(event) = self.events.pop_front() {
            return Async::Ready(event);
        }

        if let Some(r) = self.pending_replies.pop_front() {
            // The protocol names can be bytes, but the identify protocol except UTF-8 strings.
            // There's not much we can do to solve this conflict except strip non-UTF-8 characters.
            let protocols: Vec<_> = params
                .supported_protocols()
                .map(|p| String::from_utf8_lossy(&p).to_string())
                .collect();

            let mut listen_addrs: Vec<_> = params.external_addresses().collect();
            listen_addrs.extend(params.listened_addresses());

            let mut sending = 0;
            let to_send = self.pending_replies.len() + 1;
            let mut reply = Some(r);
            loop {
                match reply {
                    Some(Reply::Queued { peer, io, observed }) => {
                        let info = IdentifyInfo {
                            public_key: self.local_public_key.clone(),
                            protocol_version: self.protocol_version.clone(),
                            agent_version: self.agent_version.clone(),
                            listen_addrs: listen_addrs.clone(),
                            protocols: protocols.clone(),
                        };
                        let io = io.send(info, &observed);
                        reply = Some(Reply::Sending { peer, io });
                    }
                    Some(Reply::Sending { peer, mut io }) => {
                        sending += 1;
                        match io.poll() {
                            Ok(Async::Ready(())) => {
                                let event = IdentifyEvent::Sent { peer_id: peer };
                                return Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
                            },
                            Ok(Async::NotReady) => {
                                self.pending_replies.push_back(Reply::Sending { peer, io });
                                if sending == to_send {
                                    // All remaining futures are NotReady
                                    break
                                } else {
                                    reply = self.pending_replies.pop_front();
                                }
                            }
                            Err(err) => {
                                let event = IdentifyEvent::Error {
                                    peer_id: peer,
                                    error: ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(err))
                                };
                                return Async::Ready(NetworkBehaviourAction::GenerateEvent(event));
                            },
                        }
                    }
                    None => unreachable!()
                }
            }
        }

        Async::NotReady
    }
}

/// Event emitted  by the `Identify` behaviour.
#[derive(Debug)]
pub enum IdentifyEvent {
    /// Identifying information has been received from a peer.
    Received {
        /// The peer that has been identified.
        peer_id: PeerId,
        /// The information provided by the peer.
        info: IdentifyInfo,
        /// The address observed by the peer for the local node.
        observed_addr: Multiaddr,
    },
    /// Identifying information of the local node has been sent to a peer.
    Sent {
        /// The peer that the information has been sent to.
        peer_id: PeerId,
    },
    /// Error while attempting to identify the remote.
    Error {
        /// The peer with whom the error originated.
        peer_id: PeerId,
        /// The error that occurred.
        error: ProtocolsHandlerUpgrErr<io::Error>,
    },
}

#[cfg(test)]
mod tests {
    use crate::{Identify, IdentifyEvent};
    use futures::{future, prelude::*};
    use libp2p_core::{
        identity,
        PeerId,
        muxing::StreamMuxer,
        Multiaddr,
        Transport,
        upgrade
    };
    use libp2p_tcp::TcpConfig;
    use libp2p_secio::SecioConfig;
    use libp2p_swarm::Swarm;
    use libp2p_mplex::MplexConfig;
    use rand::{Rng, thread_rng};
    use std::{fmt, io};
    use tokio::runtime::current_thread;

    fn transport() -> (identity::PublicKey, impl Transport<
        Output = (PeerId, impl StreamMuxer<Substream = impl Send, OutboundSubstream = impl Send, Error = impl Into<io::Error>>),
        Listener = impl Send,
        ListenerUpgrade = impl Send,
        Dial = impl Send,
        Error = impl fmt::Debug
    > + Clone) {
        let id_keys = identity::Keypair::generate_ed25519();
        let pubkey = id_keys.public();
        let transport = TcpConfig::new()
            .nodelay(true)
            .upgrade(upgrade::Version::V1)
            .authenticate(SecioConfig::new(id_keys))
            .multiplex(MplexConfig::new());
        (pubkey, transport)
    }

    #[test]
    fn periodic_id_works() {
        let (mut swarm1, pubkey1) = {
            let (pubkey, transport) = transport();
            let protocol = Identify::new("a".to_string(), "b".to_string(), pubkey.clone());
            let swarm = Swarm::new(transport, protocol, pubkey.clone().into_peer_id());
            (swarm, pubkey)
        };

        let (mut swarm2, pubkey2) = {
            let (pubkey, transport) = transport();
            let protocol = Identify::new("c".to_string(), "d".to_string(), pubkey.clone());
            let swarm = Swarm::new(transport, protocol, pubkey.clone().into_peer_id());
            (swarm, pubkey)
        };

        let addr: Multiaddr = {
            let port = thread_rng().gen_range(49152, std::u16::MAX);
            format!("/ip4/127.0.0.1/tcp/{}", port).parse().unwrap()
        };

        Swarm::listen_on(&mut swarm1, addr.clone()).unwrap();
        Swarm::dial_addr(&mut swarm2, addr.clone()).unwrap();

        // nb. Either swarm may receive the `Identified` event first, upon which
        // it will permit the connection to be closed, as defined by
        // `IdentifyHandler::connection_keep_alive`. Hence the test succeeds if
        // either `Identified` event arrives correctly.
        current_thread::Runtime::new().unwrap().block_on(
            future::poll_fn(move || -> Result<_, io::Error> {
                loop {
                    match swarm1.poll().unwrap() {
                        Async::Ready(Some(IdentifyEvent::Received { info, .. })) => {
                            assert_eq!(info.public_key, pubkey2);
                            assert_eq!(info.protocol_version, "c");
                            assert_eq!(info.agent_version, "d");
                            assert!(!info.protocols.is_empty());
                            assert!(info.listen_addrs.is_empty());
                            return Ok(Async::Ready(()))
                        },
                        Async::Ready(Some(IdentifyEvent::Sent { .. })) => (),
                        Async::Ready(e) => panic!("{:?}", e),
                        Async::NotReady => {}
                    }

                    match swarm2.poll().unwrap() {
                        Async::Ready(Some(IdentifyEvent::Received { info, .. })) => {
                            assert_eq!(info.public_key, pubkey1);
                            assert_eq!(info.protocol_version, "a");
                            assert_eq!(info.agent_version, "b");
                            assert!(!info.protocols.is_empty());
                            assert_eq!(info.listen_addrs.len(), 1);
                            return Ok(Async::Ready(()))
                        },
                        Async::Ready(Some(IdentifyEvent::Sent { .. })) => (),
                        Async::Ready(e) => panic!("{:?}", e),
                        Async::NotReady => break
                    }
                }

                Ok(Async::NotReady)
            }))
            .unwrap();
    }
}
