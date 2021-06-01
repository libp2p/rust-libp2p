use crate::codec::{ErrorCode, Message, Registration, NewRegistration};
use crate::handler::{Input, RendezvousHandler};
use libp2p_core::connection::ConnectionId;
use libp2p_core::{AuthenticatedPeerRecord, Multiaddr, PeerId, PeerRecord, SignedEnvelope};
use libp2p_swarm::{
    NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters, ProtocolsHandler,
};
use log::debug;
use std::collections::{HashMap, HashSet, VecDeque};
use std::task::{Context, Poll};
use libp2p_core::identity::Keypair;

// TODO: Unit Tests
pub struct Registrations {
    registrations_for_namespace: HashMap<String, HashMap<PeerId, Registration>>,
}

impl Registrations {
    pub fn new() -> Self {
        Self {
            registrations_for_namespace: Default::default()
        }
    }

    pub fn add(&mut self, new_registration: NewRegistration) -> (String, i64) {
        let ttl = new_registration.effective_ttl();
        let namespace = new_registration.namespace;

        self.registrations_for_namespace.entry(namespace.clone())
            .or_insert_with(|| HashMap::new())
            .insert( new_registration.record.peer_id(), Registration {
                namespace: namespace.clone(),
                record: new_registration.record,
                ttl
            });

        (namespace, ttl)
    }

    pub fn remove(&mut self, namespace: String, peer_id: PeerId) {
        if let Some(registrations) = self.registrations_for_namespace.get_mut(&namespace) {
            registrations.remove(&peer_id);
        }
    }

    pub fn get(&mut self, namespace: Option<String>) -> Option<Vec<Registration>> {
        if self.registrations_for_namespace.is_empty() {
            return None;
        }

        if let Some(namespace) = namespace {
            if let Some(registrations) = self.registrations_for_namespace.get(&namespace) {
                Some(registrations.values().cloned().collect::<Vec<Registration>>())
            } else {
                None
            }
        } else {
            let discovered = self
                .registrations_for_namespace
                .iter()
                .map(|(ns, registrations)| {
                    registrations.values().cloned().collect::<Vec<Registration>>()
                })
                .flatten()
                .collect::<Vec<Registration>>();

            Some(discovered)
        }
    }
}

pub const DOMAIN: &str = "libp2p-rendezvous";

pub struct Rendezvous {
    events: VecDeque<NetworkBehaviourAction<Input, Event>>,
    registrations: Registrations,
    key_pair: Keypair,
    peer_record: PeerRecord,
}

impl Rendezvous {
    pub fn new(key_pair: Keypair, listen_addresses: Vec<Multiaddr>) -> Self {
        let peer_record = PeerRecord {
            peer_id: key_pair.public().into_peer_id(),
            seq: 0,
            addresses: listen_addresses
        };

        Self {
            events: Default::default(),
            registrations: Registrations::new(),
            key_pair,
            peer_record,
        }
    }

    pub fn register(
        &mut self,
        namespace: String,
        rendezvous_node: PeerId,
    ) {

        let authenticated_peer_record = AuthenticatedPeerRecord::from_record(self.key_pair.clone(), self.peer_record.clone());

        self.events
            .push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: rendezvous_node,
                event: Input::RegisterRequest {
                    request: NewRegistration {
                        namespace,
                        record: authenticated_peer_record,
                        ttl: None
                    }
                },
                handler: NotifyHandler::Any,
            });
    }

    pub fn unregister(&mut self, namespace: String, rendezvous_node: PeerId) {
        self.events
            .push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: rendezvous_node,
                event: Input::UnregisterRequest { namespace },
                handler: NotifyHandler::Any,
            });
    }
    pub fn discover(&mut self, ns: Option<String>, rendezvous_node: PeerId) {
        self.events
            .push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: rendezvous_node,
                event: Input::DiscoverRequest { namespace: ns },
                handler: NotifyHandler::Any,
            });
    }
}

#[derive(Debug)]
pub enum Event {
    Discovered {
        rendezvous_node: PeerId,
        registrations: Vec<Registration>,
    },
    FailedToDiscover {
        rendezvous_node: PeerId,
        err_code: ErrorCode,
    },
    RegisteredWithRendezvousNode {
        rendezvous_node: PeerId,
        ttl: i64,
        // TODO: get the namespace in as well, needs association between the registration request and the response
    },
    FailedToRegisterWithRendezvousNode {
        rendezvous_node: PeerId,
        err_code: ErrorCode,
        // TODO: get the namespace in as well, needs association between the registration request and the response
    },
    DeclinedRegisterRequest {
        peer: PeerId,
        // TODO: get the namespace in as well, needs association between the registration request and the response
    },
    PeerRegistered {
        peer_id: PeerId,
        namespace: String,
    },
    PeerUnregistered {
        peer_id: PeerId,
        namespace: String,
    },
}

impl NetworkBehaviour for Rendezvous {
    type ProtocolsHandler = RendezvousHandler;
    type OutEvent = Event;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        RendezvousHandler::new()
    }

    fn addresses_of_peer(&mut self, _: &PeerId) -> Vec<Multiaddr> {
        Vec::new()
    }

    fn inject_connected(&mut self, peer_id: &PeerId) {
        debug!("New peer connected: {}", peer_id);
        // Dont need to do anything here?
    }

    fn inject_disconnected(&mut self, peer_id: &PeerId) {
        debug!("Peer disconnected: {}", peer_id);
        // Don't need to do anything?
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        _connection: ConnectionId,
        event: crate::handler::HandlerEvent,
    ) {
        match event.0 {
            Message::Register(new_registration) => {
                let (namespace, ttl) = self.registrations.add(new_registration);

                // emit behaviour event
                self.events.push_back(NetworkBehaviourAction::GenerateEvent(Event::PeerRegistered { peer_id, namespace }));

                // notify the handler that to send a response
                self.events
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id,
                        handler: NotifyHandler::Any,
                        event: Input::RegisterResponse { ttl },
                    });
            }
            Message::RegisterResponse { ttl } => {
                self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                    Event::RegisteredWithRendezvousNode {
                        rendezvous_node: peer_id,
                        ttl,
                    },
                ))
            }
            Message::FailedToRegister { error } => {
                self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                    Event::FailedToRegisterWithRendezvousNode {
                        rendezvous_node: peer_id,
                        err_code: error,
                    },
                ))
            }
            Message::Unregister { namespace } => {
                self.registrations.remove(namespace, peer_id);
                // TODO: Should send unregister response?
            }
            Message::Discover { namespace } => {

                let registrations = self.registrations.get(namespace);

                if let Some(registrations) = registrations {
                    self.events
                        .push_back(NetworkBehaviourAction::NotifyHandler {
                            peer_id,
                            handler: NotifyHandler::Any,
                            event: Input::DiscoverResponse {
                                discovered: registrations
                            }})
                } else {
                    self.events
                        .push_back(NetworkBehaviourAction::NotifyHandler {
                            peer_id,
                            handler: NotifyHandler::Any,
                            event: Input::DiscoverResponse { discovered: vec![] },
                        })
                }
            }
            Message::DiscoverResponse { registrations } => {
                self.events
                    .push_back(NetworkBehaviourAction::GenerateEvent(Event::Discovered {
                        rendezvous_node: peer_id,
                        registrations,
                    }))
            }
            Message::FailedToDiscover { error } => self.events.push_back(
                NetworkBehaviourAction::GenerateEvent(Event::FailedToDiscover {
                    rendezvous_node: peer_id,
                    err_code: error,
                }),
            ),
        }
    }

    fn poll(
        &mut self,
        _cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}
