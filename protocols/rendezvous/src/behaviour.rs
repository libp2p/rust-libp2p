use crate::codec::{Cookie, ErrorCode, Message, NewRegistration, Registration};
use crate::handler;
use crate::handler::{InEvent, RendezvousHandler};
use libp2p_core::connection::ConnectionId;
use libp2p_core::identity::Keypair;
use libp2p_core::{AuthenticatedPeerRecord, Multiaddr, PeerId, PeerRecord};
use libp2p_swarm::{
    NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters, ProtocolsHandler,
};
use log::debug;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::task::{Context, Poll};
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};
use uuid::Uuid;

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
        cookie: Cookie,
    },
    AnsweredDiscoverRequest {
        enquirer: PeerId,
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
                let namespace = new_registration.namespace.clone();

                if let Ok(effective_ttl) = self.registrations.add(new_registration) {
                    // notify the handler that to send a response
                    self.events
                        .push_back(NetworkBehaviourAction::NotifyHandler {
                            peer_id,
                            handler: NotifyHandler::Any,
                            event: InEvent::RegisterResponse { ttl: effective_ttl },
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
            Message::Discover { namespace, cookie } => {
                let (registrations, cookie) = self.registrations.get(namespace, cookie);

                let discovered = registrations.cloned().collect::<Vec<_>>();

                self.events
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id,
                        handler: NotifyHandler::Any,
                        event: InEvent::DiscoverResponse {
                            discovered: discovered.clone(),
                            cookie,
                        },
                    });
                self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                    Event::AnsweredDiscoverRequest {
                        enquirer: peer_id,
                        registrations: discovered,
                    },
                ));
            }
            Message::DiscoverResponse {
                registrations,
                cookie,
            } => self
                .events
                .push_back(NetworkBehaviourAction::GenerateEvent(Event::Discovered {
                    rendezvous_node: peer_id,
                    registrations: registrations
                        .iter()
                        .map(|r| ((r.namespace.clone(), r.record.peer_id()), r.clone()))
                        .collect(),
                    cookie,
                })),
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

#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
struct RegistrationId(Uuid);

impl RegistrationId {
    fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

// TODO: Unit Tests
pub struct Registrations {
    registrations_for_peer: HashMap<(PeerId, String), RegistrationId>,
    registrations: HashMap<RegistrationId, Registration>,
    cookies: HashMap<Cookie, Vec<RegistrationId>>,
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
            registrations_for_peer: Default::default(),
            registrations: Default::default(),
            ttl_upper_bound,
            cookies: Default::default(),
        }
    }

    pub fn add(&mut self, new_registration: NewRegistration) -> Result<i64, RegistrationError> {
        let ttl = new_registration.effective_ttl();
        if ttl < self.ttl_upper_bound {
            let namespace = new_registration.namespace;
            let registration_id = RegistrationId::new();

            match self
                .registrations_for_peer
                .entry((new_registration.record.peer_id(), namespace.clone()))
            {
                Entry::Occupied(mut occupied) => {
                    let old_registration = occupied.insert(registration_id);

                    self.registrations.remove(&old_registration);
                }
                Entry::Vacant(vacant) => {
                    vacant.insert(registration_id);
                }
            }

            self.registrations.insert(
                registration_id,
                Registration {
                    namespace: namespace.clone(),
                    record: new_registration.record,
                    ttl,
                    timestamp: SystemTime::now(),
                },
            );
            Ok(ttl)
        } else {
            Err(RegistrationError::TTLGreaterThanUpperBound {
                upper_bound: self.ttl_upper_bound,
                requested: ttl,
            })
        }
    }

    pub fn remove(&mut self, namespace: String, peer_id: PeerId) {
        let reggo_to_remove = self.registrations_for_peer.remove(&(peer_id, namespace));

        if let Some(reggo_to_remove) = reggo_to_remove {
            self.registrations.remove(&reggo_to_remove);
        }
    }

    pub fn get(
        &mut self,
        discover_namespace: Option<String>,
        cookie: Option<Cookie>,
    ) -> (impl Iterator<Item = &Registration> + '_, Cookie) {
        let mut reggos_of_last_discover = cookie
            .and_then(|cookie| self.cookies.get(&cookie))
            .cloned()
            .unwrap_or_default();

        let ids = self
            .registrations_for_peer
            .iter()
            .filter_map({
                |((_, namespace), registration_id)| {
                    if reggos_of_last_discover.contains(registration_id) {
                        return None;
                    }

                    match discover_namespace.as_ref() {
                        Some(discover_namespace) if discover_namespace == namespace => {
                            Some(registration_id)
                        }
                        Some(_) => None,
                        None => Some(registration_id),
                    }
                }
            })
            .cloned()
            .collect::<Vec<_>>();

        reggos_of_last_discover.extend_from_slice(&ids);

        let new_cookie = Cookie::new();
        self.cookies.insert(new_cookie, reggos_of_last_discover);

        let reggos = &self.registrations;
        let registrations = ids
            .into_iter()
            .map(move |id| reggos.get(&id).expect("bad internal datastructure"));

        (registrations, new_cookie)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::identity;

    #[test]
    fn given_cookie_from_discover_when_discover_again_then_only_get_diff() {
        let mut registrations = Registrations::new(7201);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("foo")).unwrap();

        let (initial_discover, cookie) = registrations.get(None, None);
        assert_eq!(initial_discover.collect::<Vec<_>>().len(), 2);

        let (subsequent_discover, _) = registrations.get(None, Some(cookie));
        assert_eq!(subsequent_discover.collect::<Vec<_>>().len(), 0);
    }

    #[test]
    fn given_registrations_when_discover_all_then_all_are_returned() {
        let mut registrations = Registrations::new(7201);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("foo")).unwrap();

        let (discover, _) = registrations.get(None, None);

        assert_eq!(discover.collect::<Vec<_>>().len(), 2);
    }

    #[test]
    fn given_registrations_when_discover_only_for_specific_namespace_then_only_those_are_returned()
    {
        let mut registrations = Registrations::new(7201);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("bar")).unwrap();

        let (discover, _) = registrations.get(Some("foo".to_owned()), None);

        assert_eq!(
            discover.map(|r| r.namespace.as_str()).collect::<Vec<_>>(),
            vec!["foo"]
        );
    }

    #[test]
    fn given_reregistration_old_registration_is_discarded() {
        let alice = identity::Keypair::generate_ed25519();
        let mut registrations = Registrations::new(7201);
        registrations
            .add(new_registration("foo", alice.clone()))
            .unwrap();
        registrations.add(new_registration("foo", alice)).unwrap();

        let (discover, _) = registrations.get(Some("foo".to_owned()), None);

        assert_eq!(
            discover.map(|r| r.namespace.as_str()).collect::<Vec<_>>(),
            vec!["foo"]
        );
    }

    #[test]
    fn given_cookie_from_2nd_discover_does_not_return_nodes_from_first_discover() {
        let mut registrations = Registrations::new(7201);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("foo")).unwrap();

        let (initial_discover, cookie1) = registrations.get(None, None);
        assert_eq!(initial_discover.collect::<Vec<_>>().len(), 2);

        let (subsequent_discover, cookie2) = registrations.get(None, Some(cookie1));
        assert_eq!(subsequent_discover.collect::<Vec<_>>().len(), 0);

        let (subsequent_discover, _) = registrations.get(None, Some(cookie2));
        assert_eq!(subsequent_discover.collect::<Vec<_>>().len(), 0);
    }

    fn new_dummy_registration(namespace: &str) -> NewRegistration {
        let identity = identity::Keypair::generate_ed25519();

        new_registration(namespace, identity)
    }

    fn new_registration(namespace: &str, identity: identity::Keypair) -> NewRegistration {
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
