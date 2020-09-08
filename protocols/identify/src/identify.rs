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
use crate::protocol::{IdentifyInfo, ReplySubstream};
use futures::prelude::*;
use libp2p_core::{
    ConnectedPoint,
    Multiaddr,
    PeerId,
    PublicKey,
    connection::ConnectionId,
    upgrade::{ReadOneError, UpgradeError}
};
use libp2p_swarm::{
    NegotiatedSubstream,
    NetworkBehaviour,
    NetworkBehaviourAction,
    PollParameters,
    ProtocolsHandler,
    ProtocolsHandlerUpgrErr
};
use std::{
    collections::{HashMap, VecDeque},
    io,
    pin::Pin,
    task::Context,
    task::Poll
};

/// Network behaviour that automatically identifies nodes periodically, returns information
/// about them, and answers identify queries from other nodes.
pub struct Identify {
    /// Protocol version to send back to remotes.
    protocol_version: String,
    /// Agent version to send back to remotes.
    agent_version: String,
    /// The public key of the local node. To report on the wire.
    local_public_key: PublicKey,
    /// For each peer we're connected to, the observed address to send back to it.
    observed_addresses: HashMap<PeerId, HashMap<ConnectionId, Multiaddr>>,
    /// Pending replies to send.
    pending_replies: VecDeque<Reply>,
    /// Pending events to be emitted when polled.
    events: VecDeque<NetworkBehaviourAction<(), IdentifyEvent>>,
}

/// A pending reply to an inbound identification request.
enum Reply {
    /// The reply is queued for sending.
    Queued {
        peer: PeerId,
        io: ReplySubstream<NegotiatedSubstream>,
        observed: Multiaddr
    },
    /// The reply is being sent.
    Sending {
        peer: PeerId,
        io: Pin<Box<dyn Future<Output = Result<(), io::Error>> + Send>>,
    }
}

impl Identify {
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

impl NetworkBehaviour for Identify {
    type ProtocolsHandler = IdentifyHandler;
    type OutEvent = IdentifyEvent;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        IdentifyHandler::new()
    }

    fn addresses_of_peer(&mut self, _: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, _: &PeerId) {
    }

    fn inject_connection_established(&mut self, peer_id: &PeerId, conn: &ConnectionId, endpoint: &ConnectedPoint) {
        let addr = match endpoint {
            ConnectedPoint::Dialer { address } => address.clone(),
            ConnectedPoint::Listener { send_back_addr, .. } => send_back_addr.clone(),
        };

        self.observed_addresses.entry(peer_id.clone()).or_default().insert(*conn, addr);
    }

    fn inject_connection_closed(&mut self, peer_id: &PeerId, conn: &ConnectionId, _: &ConnectedPoint) {
        if let Some(addrs) = self.observed_addresses.get_mut(peer_id) {
            addrs.remove(conn);
        }
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        self.observed_addresses.remove(peer_id);
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        connection: ConnectionId,
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
                    .and_then(|addrs| addrs.get(&connection))
                    .expect("`inject_event` is only called with an established connection \
                             and `inject_connection_established` ensures there is an entry; qed");
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
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
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
                        let io = Box::pin(io.send(info, &observed));
                        reply = Some(Reply::Sending { peer, io });
                    }
                    Some(Reply::Sending { peer, mut io }) => {
                        sending += 1;
                        match Future::poll(Pin::new(&mut io), cx) {
                            Poll::Ready(Ok(())) => {
                                let event = IdentifyEvent::Sent { peer_id: peer };
                                return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
                            },
                            Poll::Pending => {
                                self.pending_replies.push_back(Reply::Sending { peer, io });
                                if sending == to_send {
                                    // All remaining futures are NotReady
                                    break
                                } else {
                                    reply = self.pending_replies.pop_front();
                                }
                            }
                            Poll::Ready(Err(err)) => {
                                let event = IdentifyEvent::Error {
                                    peer_id: peer,
                                    error: ProtocolsHandlerUpgrErr::Upgrade(UpgradeError::Apply(err.into()))
                                };
                                return Poll::Ready(NetworkBehaviourAction::GenerateEvent(event));
                            },
                        }
                    }
                    None => unreachable!()
                }
            }
        }

        Poll::Pending
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
        error: ProtocolsHandlerUpgrErr<ReadOneError>,
    },
}

#[cfg(test)]
mod tests {
    use crate::{Identify, IdentifyEvent};
    use futures::{prelude::*, pin_mut};
    use libp2p_core::{
        identity,
        PeerId,
        muxing::StreamMuxer,
        Transport,
        upgrade
    };
    use libp2p_noise as noise;
    use libp2p_tcp::TcpConfig;
    use libp2p_swarm::{Swarm, SwarmEvent};
    use libp2p_mplex::MplexConfig;
    use std::{fmt, io};

    fn transport() -> (identity::PublicKey, impl Transport<
        Output = (PeerId, impl StreamMuxer<Substream = impl Send, OutboundSubstream = impl Send, Error = impl Into<io::Error>>),
        Listener = impl Send,
        ListenerUpgrade = impl Send,
        Dial = impl Send,
        Error = impl fmt::Debug
    > + Clone) {
        let id_keys = identity::Keypair::generate_ed25519();
        let noise_keys = noise::Keypair::<noise::X25519Spec>::new().into_authentic(&id_keys).unwrap();
        let pubkey = id_keys.public();
        let transport = TcpConfig::new()
            .nodelay(true)
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
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

        Swarm::listen_on(&mut swarm1, "/ip4/127.0.0.1/tcp/0".parse().unwrap()).unwrap();

        let listen_addr = async_std::task::block_on(async {
            loop {
                let swarm1_fut = swarm1.next_event();
                pin_mut!(swarm1_fut);
                match swarm1_fut.await {
                    SwarmEvent::NewListenAddr(addr) => return addr,
                    _ => {}
                }
            }
        });
        Swarm::dial_addr(&mut swarm2, listen_addr).unwrap();

        // nb. Either swarm may receive the `Identified` event first, upon which
        // it will permit the connection to be closed, as defined by
        // `IdentifyHandler::connection_keep_alive`. Hence the test succeeds if
        // either `Identified` event arrives correctly.
        async_std::task::block_on(async move {
            loop {
                let swarm1_fut = swarm1.next();
                pin_mut!(swarm1_fut);
                let swarm2_fut = swarm2.next();
                pin_mut!(swarm2_fut);

                match future::select(swarm1_fut, swarm2_fut).await.factor_second().0 {
                    future::Either::Left(IdentifyEvent::Received { info, .. }) => {
                        assert_eq!(info.public_key, pubkey2);
                        assert_eq!(info.protocol_version, "c");
                        assert_eq!(info.agent_version, "d");
                        assert!(!info.protocols.is_empty());
                        assert!(info.listen_addrs.is_empty());
                        return;
                    }
                    future::Either::Right(IdentifyEvent::Received { info, .. }) => {
                        assert_eq!(info.public_key, pubkey1);
                        assert_eq!(info.protocol_version, "a");
                        assert_eq!(info.agent_version, "b");
                        assert!(!info.protocols.is_empty());
                        assert_eq!(info.listen_addrs.len(), 1);
                        return;
                    }
                    _ => {}
                }
            }
        })
    }
}
