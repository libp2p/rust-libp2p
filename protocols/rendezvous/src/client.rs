// Copyright 2021 COMIT Network.
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

use crate::codec::{Cookie, ErrorCode, Namespace, NewRegistration, Registration, Ttl};
use crate::handler;
use crate::handler::outbound;
use crate::handler::outbound::OpenInfo;
use crate::substream_handler::SubstreamConnectionHandler;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use instant::Duration;
use libp2p_core::connection::ConnectionId;
use libp2p_core::identity::error::SigningError;
use libp2p_core::identity::Keypair;
use libp2p_core::{Multiaddr, PeerId, PeerRecord};
use libp2p_swarm::{
    CloseConnection, NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters,
};
use std::collections::{HashMap, VecDeque};
use std::iter::FromIterator;
use std::task::{Context, Poll};

pub struct Behaviour {
    events: VecDeque<
        NetworkBehaviourAction<
            Event,
            SubstreamConnectionHandler<void::Void, outbound::Stream, outbound::OpenInfo>,
        >,
    >,
    keypair: Keypair,
    pending_register_requests: Vec<(Namespace, PeerId, Option<Ttl>)>,

    /// Hold addresses of all peers that we have discovered so far.
    ///
    /// Storing these internally allows us to assist the [`libp2p_swarm::Swarm`] in dialing by returning addresses from [`NetworkBehaviour::addresses_of_peer`].
    discovered_peers: HashMap<(PeerId, Namespace), Vec<Multiaddr>>,

    /// Tracks the expiry of registrations that we have discovered and stored in `discovered_peers` otherwise we have a memory leak.
    expiring_registrations: FuturesUnordered<BoxFuture<'static, (PeerId, Namespace)>>,
}

impl Behaviour {
    /// Create a new instance of the rendezvous [`NetworkBehaviour`].
    pub fn new(keypair: Keypair) -> Self {
        Self {
            events: Default::default(),
            keypair,
            pending_register_requests: vec![],
            discovered_peers: Default::default(),
            expiring_registrations: FuturesUnordered::from_iter(vec![
                futures::future::pending().boxed()
            ]),
        }
    }

    /// Register our external addresses in the given namespace with the given rendezvous peer.
    ///
    /// External addresses are either manually added via [`libp2p_swarm::Swarm::add_external_address`] or reported
    /// by other [`NetworkBehaviour`]s via [`NetworkBehaviourAction::ReportObservedAddr`].
    pub fn register(&mut self, namespace: Namespace, rendezvous_node: PeerId, ttl: Option<Ttl>) {
        self.pending_register_requests
            .push((namespace, rendezvous_node, ttl));
    }

    /// Unregister ourselves from the given namespace with the given rendezvous peer.
    pub fn unregister(&mut self, namespace: Namespace, rendezvous_node: PeerId) {
        self.events
            .push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: rendezvous_node,
                event: handler::OutboundInEvent::NewSubstream {
                    open_info: OpenInfo::UnregisterRequest(namespace),
                },
                handler: NotifyHandler::Any,
            });
    }

    /// Discover other peers at a given rendezvous peer.
    ///
    /// If desired, the registrations can be filtered by a namespace.
    /// If no namespace is given, peers from all namespaces will be returned.
    /// A successfully discovery returns a cookie within [`Event::Discovered`].
    /// Such a cookie can be used to only fetch the _delta_ of registrations since
    /// the cookie was acquired.
    pub fn discover(
        &mut self,
        ns: Option<Namespace>,
        cookie: Option<Cookie>,
        limit: Option<u64>,
        rendezvous_node: PeerId,
    ) {
        self.events
            .push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: rendezvous_node,
                event: handler::OutboundInEvent::NewSubstream {
                    open_info: OpenInfo::DiscoverRequest {
                        namespace: ns,
                        cookie,
                        limit,
                    },
                },
                handler: NotifyHandler::Any,
            });
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegisterError {
    #[error("We don't know about any externally reachable addresses of ours")]
    NoExternalAddresses,
    #[error("Failed to make a new PeerRecord")]
    FailedToMakeRecord(#[from] SigningError),
    #[error("Failed to register with Rendezvous node")]
    Remote {
        rendezvous_node: PeerId,
        namespace: Namespace,
        error: ErrorCode,
    },
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Event {
    /// We successfully discovered other nodes with using the contained rendezvous node.
    Discovered {
        rendezvous_node: PeerId,
        registrations: Vec<Registration>,
        cookie: Cookie,
    },
    /// We failed to discover other nodes on the contained rendezvous node.
    DiscoverFailed {
        rendezvous_node: PeerId,
        namespace: Option<Namespace>,
        error: ErrorCode,
    },
    /// We successfully registered with the contained rendezvous node.
    Registered {
        rendezvous_node: PeerId,
        ttl: Ttl,
        namespace: Namespace,
    },
    /// We failed to register with the contained rendezvous node.
    RegisterFailed(RegisterError),
    /// The connection details we learned from this node expired.
    Expired { peer: PeerId },
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler =
        SubstreamConnectionHandler<void::Void, outbound::Stream, outbound::OpenInfo>;
    type OutEvent = Event;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        let initial_keep_alive = Duration::from_secs(30);

        SubstreamConnectionHandler::new_outbound_only(initial_keep_alive)
    }

    fn addresses_of_peer(&mut self, peer: &PeerId) -> Vec<Multiaddr> {
        self.discovered_peers
            .iter()
            .filter_map(|((candidate, _), addresses)| (candidate == peer).then(|| addresses))
            .flatten()
            .cloned()
            .collect()
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: handler::OutboundOutEvent,
    ) {
        let new_events = match event {
            handler::OutboundOutEvent::InboundEvent { message, .. } => void::unreachable(message),
            handler::OutboundOutEvent::OutboundEvent { message, .. } => handle_outbound_event(
                message,
                peer_id,
                &mut self.discovered_peers,
                &mut self.expiring_registrations,
            ),
            handler::OutboundOutEvent::InboundError { error, .. } => void::unreachable(error),
            handler::OutboundOutEvent::OutboundError { error, .. } => {
                log::warn!("Connection with peer {} failed: {}", peer_id, error);

                vec![NetworkBehaviourAction::CloseConnection {
                    peer_id,
                    connection: CloseConnection::One(connection_id),
                }]
            }
        };

        self.events.extend(new_events);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        poll_params: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        if let Some((namespace, rendezvous_node, ttl)) = self.pending_register_requests.pop() {
            // Update our external addresses based on the Swarm's current knowledge.
            // It doesn't make sense to register addresses on which we are not reachable, hence this should not be configurable from the outside.
            let external_addresses = poll_params
                .external_addresses()
                .map(|r| r.addr)
                .collect::<Vec<Multiaddr>>();

            if external_addresses.is_empty() {
                return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                    Event::RegisterFailed(RegisterError::NoExternalAddresses),
                ));
            }

            let action = match PeerRecord::new(&self.keypair, external_addresses) {
                Ok(peer_record) => NetworkBehaviourAction::NotifyHandler {
                    peer_id: rendezvous_node,
                    event: handler::OutboundInEvent::NewSubstream {
                        open_info: OpenInfo::RegisterRequest(NewRegistration {
                            namespace,
                            record: peer_record,
                            ttl,
                        }),
                    },
                    handler: NotifyHandler::Any,
                },
                Err(signing_error) => NetworkBehaviourAction::GenerateEvent(Event::RegisterFailed(
                    RegisterError::FailedToMakeRecord(signing_error),
                )),
            };

            return Poll::Ready(action);
        }

        if let Some(expired_registration) =
            futures::ready!(self.expiring_registrations.poll_next_unpin(cx))
        {
            self.discovered_peers.remove(&expired_registration);
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(Event::Expired {
                peer: expired_registration.0,
            }));
        }

        Poll::Pending
    }
}

fn handle_outbound_event(
    event: outbound::OutEvent,
    peer_id: PeerId,
    discovered_peers: &mut HashMap<(PeerId, Namespace), Vec<Multiaddr>>,
    expiring_registrations: &mut FuturesUnordered<BoxFuture<'static, (PeerId, Namespace)>>,
) -> Vec<
    NetworkBehaviourAction<
        Event,
        SubstreamConnectionHandler<void::Void, outbound::Stream, outbound::OpenInfo>,
    >,
> {
    match event {
        outbound::OutEvent::Registered { namespace, ttl } => {
            vec![NetworkBehaviourAction::GenerateEvent(Event::Registered {
                rendezvous_node: peer_id,
                ttl,
                namespace,
            })]
        }
        outbound::OutEvent::RegisterFailed(namespace, error) => {
            vec![NetworkBehaviourAction::GenerateEvent(
                Event::RegisterFailed(RegisterError::Remote {
                    rendezvous_node: peer_id,
                    namespace,
                    error,
                }),
            )]
        }
        outbound::OutEvent::Discovered {
            registrations,
            cookie,
        } => {
            discovered_peers.extend(registrations.iter().map(|registration| {
                let peer_id = registration.record.peer_id();
                let namespace = registration.namespace.clone();

                let addresses = registration.record.addresses().to_vec();

                ((peer_id, namespace), addresses)
            }));
            expiring_registrations.extend(registrations.iter().cloned().map(|registration| {
                async move {
                    // if the timer errors we consider it expired
                    let _ = futures_timer::Delay::new(Duration::from_secs(registration.ttl as u64))
                        .await;

                    (registration.record.peer_id(), registration.namespace)
                }
                .boxed()
            }));

            vec![NetworkBehaviourAction::GenerateEvent(Event::Discovered {
                rendezvous_node: peer_id,
                registrations,
                cookie,
            })]
        }
        outbound::OutEvent::DiscoverFailed { namespace, error } => {
            vec![NetworkBehaviourAction::GenerateEvent(
                Event::DiscoverFailed {
                    rendezvous_node: peer_id,
                    namespace,
                    error,
                },
            )]
        }
    }
}
