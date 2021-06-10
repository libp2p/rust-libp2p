use crate::codec::{ErrorCode, Message, NewRegistration, Registration};
use crate::handler;
use crate::handler::{InEvent, RendezvousHandler};
use libp2p_core::connection::ConnectionId;
use libp2p_core::identity::Keypair;
use libp2p_core::{AuthenticatedPeerRecord, Multiaddr, PeerId, PeerRecord};
use libp2p_swarm::{
    NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters, ProtocolsHandler,
};
use log::debug;
use std::collections::{HashMap, VecDeque};
use std::task::{Context, Poll};
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

pub struct Rendezvous {
    events: VecDeque<NetworkBehaviourAction<InEvent, Event>>,
    registrations: Registrations,
    key_pair: Keypair,
    external_addresses: Vec<Multiaddr>,
}

impl Rendezvous {
    pub fn new(key_pair: Keypair, ttl_upper_board: i64) -> Self {
        Self {
            events: Default::default(),
            registrations: Registrations::new(ttl_upper_board),
            key_pair,
            external_addresses: vec![],
        }
    }

    // TODO: Make it possible to filter for specific external-addresses (like onion addresses-only f.e.)
    pub fn register(
        &mut self,
        namespace: String,
        rendezvous_node: PeerId,
        ttl: Option<i64>,
    ) -> Result<(), SystemTimeError> {
        let authenticated_peer_record = AuthenticatedPeerRecord::from_record(
            self.key_pair.clone(),
            PeerRecord {
                peer_id: self.key_pair.public().into_peer_id(),
                seq: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
                addresses: self.external_addresses.clone(),
            },
        );

        self.events
            .push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: rendezvous_node,
                event: InEvent::RegisterRequest {
                    request: NewRegistration {
                        namespace,
                        record: authenticated_peer_record,
                        ttl,
                    },
                },
                handler: NotifyHandler::Any,
            });

        Ok(())
    }

    pub fn unregister(&mut self, namespace: String, rendezvous_node: PeerId) {
        self.events
            .push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: rendezvous_node,
                event: InEvent::UnregisterRequest { namespace },
                handler: NotifyHandler::Any,
            });
    }

    pub fn discover(&mut self, ns: Option<String>, rendezvous_node: PeerId) {
        self.events
            .push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: rendezvous_node,
                event: InEvent::DiscoverRequest { namespace: ns },
                handler: NotifyHandler::Any,
            });
    }
}

#[derive(Debug)]
pub enum Event {
    Discovered {
        rendezvous_node: PeerId,
        registrations: HashMap<(String, PeerId), Registration>,
    },
    AnsweredDiscoverRequest {
        enquirer: PeerId,
        registrations: HashMap<(String, PeerId), Registration>,
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
        peer: PeerId,
        namespace: String,
    },
    PeerUnregistered {
        peer: PeerId,
        namespace: String,
    },
}

impl NetworkBehaviour for Rendezvous {
    type ProtocolsHandler = RendezvousHandler;
    type OutEvent = Event;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        debug!("spawning protocol handler");
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
        message: handler::OutEvent,
    ) {
        match message {
            Message::Register(new_registration) => {
                if let Ok((namespace, ttl)) = self.registrations.add(new_registration) {
                    // notify the handler that to send a response
                    self.events
                        .push_back(NetworkBehaviourAction::NotifyHandler {
                            peer_id,
                            handler: NotifyHandler::Any,
                            event: InEvent::RegisterResponse { ttl },
                        });

                    // emit behaviour event
                    self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                        Event::PeerRegistered {
                            peer: peer_id,
                            namespace,
                        },
                    ));
                } else {
                    self.events
                        .push_back(NetworkBehaviourAction::NotifyHandler {
                            peer_id,
                            handler: NotifyHandler::Any,
                            event: InEvent::DeclineRegisterRequest {
                                error: ErrorCode::InvalidTtl,
                            },
                        });

                    self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                        Event::DeclinedRegisterRequest { peer: peer_id },
                    ));
                }
            }
            Message::RegisterResponse { ttl } => self.events.push_back(
                NetworkBehaviourAction::GenerateEvent(Event::RegisteredWithRendezvousNode {
                    rendezvous_node: peer_id,
                    ttl,
                }),
            ),
            Message::FailedToRegister { error } => self.events.push_back(
                NetworkBehaviourAction::GenerateEvent(Event::FailedToRegisterWithRendezvousNode {
                    rendezvous_node: peer_id,
                    err_code: error,
                }),
            ),
            Message::Unregister { namespace } => {
                self.registrations.remove(namespace, peer_id);
                // TODO: Should send unregister response?
            }
            Message::Discover { namespace } => {
                let (registrations, _) = self.registrations.get(namespace, None);

                self.events
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id,
                        handler: NotifyHandler::Any,
                        event: InEvent::DiscoverResponse {
                            discovered: registrations.values().cloned().collect(),
                        },
                    });
                self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                    Event::AnsweredDiscoverRequest {
                        enquirer: peer_id,
                        registrations,
                    },
                ));
            }
            Message::DiscoverResponse { registrations } => {
                self.events
                    .push_back(NetworkBehaviourAction::GenerateEvent(Event::Discovered {
                        rendezvous_node: peer_id,
                        registrations: registrations
                            .iter()
                            .map(|r| ((r.namespace.clone(), r.record.peer_id()), r.clone()))
                            .collect(),
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
        poll_params: &mut impl PollParameters,
    ) -> Poll<
        NetworkBehaviourAction<
            <Self::ProtocolsHandler as ProtocolsHandler>::InEvent,
            Self::OutEvent,
        >,
    > {
        // Update our external addresses based on the Swarm's current knowledge.
        // It doesn't make sense to register addresses on which we are not reachable, hence this should not be configurable from the outside.
        self.external_addresses = poll_params.external_addresses().map(|r| r.addr).collect();

        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        Poll::Pending
    }
}

pub struct Cookie {}

// TODO: Unit Tests
pub struct Registrations {
    registrations_for_namespace: HashMap<String, HashMap<PeerId, Registration>>,
    ttl_upper_bound: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum RegistrationError {
    #[error("Requested TTL: {requested} is greater than upper bound: {upper_bound} ")]
    TTLGreaterThanUpperBound { upper_bound: i64, requested: i64 },
}

impl Registrations {
    pub fn new(ttl_upper_bound: i64) -> Self {
        Self {
            registrations_for_namespace: Default::default(),
            ttl_upper_bound,
        }
    }

    pub fn add(
        &mut self,
        new_registration: NewRegistration,
    ) -> Result<(String, i64), RegistrationError> {
        let ttl = new_registration.effective_ttl();
        if ttl < self.ttl_upper_bound {
            let namespace = new_registration.namespace;
            self.registrations_for_namespace
                .entry(namespace.clone())
                .or_insert_with(|| HashMap::new())
                .insert(
                    new_registration.record.peer_id(),
                    Registration {
                        namespace: namespace.clone(),
                        record: new_registration.record,
                        ttl,
                        timestamp: SystemTime::now(),
                    },
                );
            Ok((namespace, ttl))
        } else {
            Err(RegistrationError::TTLGreaterThanUpperBound {
                upper_bound: self.ttl_upper_bound,
                requested: ttl,
            })
        }
    }

    pub fn remove(&mut self, namespace: String, peer_id: PeerId) {
        if let Some(registrations) = self.registrations_for_namespace.get_mut(&namespace) {
            registrations.remove(&peer_id);
        }
    }

    pub fn get(
        &mut self,
        namespace: Option<String>,
        _: Option<Cookie>,
    ) -> (HashMap<(String, PeerId), Registration>, Cookie) {
        if self.registrations_for_namespace.is_empty() {
            return (HashMap::new(), Cookie {});
        }

        let discovered = if let Some(namespace) = namespace {
            if let Some(registrations) = self.registrations_for_namespace.get(&namespace) {
                registrations
                    .values()
                    .map(|r| ((r.namespace.clone(), r.record.peer_id()), r.clone()))
                    .collect::<HashMap<(String, PeerId), Registration>>()
            } else {
                HashMap::new()
            }
        } else {
            let discovered = self
                .registrations_for_namespace
                .iter()
                .map(|(_, registrations)| {
                    registrations
                        .values()
                        .map(|r| ((r.namespace.clone(), r.record.peer_id()), r.clone()))
                        .collect::<HashMap<(String, PeerId), Registration>>()
                })
                .flatten()
                .collect::<HashMap<(String, PeerId), Registration>>();

            discovered
        };

        (discovered, Cookie {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::identity;

    #[test]
    fn given_cookie_from_discover_when_discover_again_then_only_get_diff() {
        let mut registrations = Registrations::new();
        registrations.add(new_dummy_registration("foo"));
        registrations.add(new_dummy_registration("foo"));

        let (initial_discover, cookie) = registrations.get(None, None);
        let (subsequent_discover, _) = registrations.get(None, Some(cookie));

        assert_eq!(initial_discover.len(), 2);
        assert_eq!(subsequent_discover.len(), 0);
    }

    fn new_dummy_registration(namespace: &str) -> NewRegistration {
        let identity = identity::Keypair::generate_ed25519();
        let record = PeerRecord {
            peer_id: identity.public().into_peer_id(),
            seq: 0,
            addresses: vec!["/ip4/127.0.0.1/tcp/1234".parse().unwrap()],
        };

        NewRegistration::new(
            namespace.to_owned(),
            AuthenticatedPeerRecord::from_record(identity, record),
            None,
        )
    }
}
