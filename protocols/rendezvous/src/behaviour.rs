use crate::codec::{Cookie, ErrorCode, Message, NewRegistration, Registration};
use crate::handler;
use crate::handler::{InEvent, RendezvousHandler};
use libp2p_core::connection::ConnectionId;
use libp2p_core::identity::error::SigningError;
use libp2p_core::identity::Keypair;
use libp2p_core::{Multiaddr, PeerId, PeerRecord};
use libp2p_swarm::{
    NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters, ProtocolsHandler,
};
use log::debug;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::task::{Context, Poll};
use std::time::SystemTime;
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
    ) -> Result<(), RegisterError> {
        if self.external_addresses.is_empty() {
            return Err(RegisterError::NoExternalAddresses);
        }

        let peer_record = PeerRecord::new(self.key_pair.clone(), self.external_addresses.clone())?;

        self.events
            .push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: rendezvous_node,
                event: InEvent::RegisterRequest {
                    request: NewRegistration {
                        namespace,
                        record: peer_record,
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

    pub fn discover(
        &mut self,
        ns: Option<String>,
        cookie: Option<Cookie>,
        rendezvous_node: PeerId,
    ) {
        self.events
            .push_back(NetworkBehaviourAction::NotifyHandler {
                peer_id: rendezvous_node,
                event: InEvent::DiscoverRequest {
                    namespace: ns,
                    cookie,
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
}

#[derive(Debug)]
pub enum Event {
    /// We successfully discovered other nodes with using the contained rendezvous node.
    Discovered {
        rendezvous_node: PeerId,
        registrations: HashMap<(String, PeerId), Registration>,
        cookie: Cookie,
    },
    /// We failed to discover other nodes on the contained rendezvous node.
    FailedToDiscover {
        rendezvous_node: PeerId,
        err_code: ErrorCode,
    },
    /// We successfully registered with the contained rendezvous node.
    Registered {
        rendezvous_node: PeerId,
        ttl: i64,
        // TODO: get the namespace in as well, needs association between the registration request and the response
    },
    /// We failed to register with the contained rendezvous node.
    FailedToRegister {
        rendezvous_node: PeerId,
        err_code: ErrorCode,
        // TODO: get the namespace in as well, needs association between the registration request and the response
    },
    /// We successfully served a discover request from a peer.
    AnsweredDiscoverRequest {
        enquirer: PeerId,
        registrations: Vec<Registration>,
    },
    /// We declined a registration from a peer.
    DeclinedRegisterRequest {
        peer: PeerId,
        // TODO: get the namespace in as well, needs association between the registration request and the response
    },
    /// A peer successfully registered with us.
    // TODO: Include registration here
    PeerRegistered { peer: PeerId, namespace: String },
    /// A peer successfully unregistered with us.
    PeerUnregistered { peer: PeerId, namespace: String },
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

                let events = match self.registrations.add(new_registration) {
                    Ok(effective_ttl) => {
                        vec![
                            NetworkBehaviourAction::NotifyHandler {
                                peer_id,
                                handler: NotifyHandler::Any,
                                event: InEvent::RegisterResponse { ttl: effective_ttl },
                            },
                            NetworkBehaviourAction::GenerateEvent(Event::PeerRegistered {
                                peer: peer_id,
                                namespace,
                            }),
                        ]
                    }
                    Err(TtlTooLong { .. }) => {
                        vec![
                            NetworkBehaviourAction::NotifyHandler {
                                peer_id,
                                handler: NotifyHandler::Any,
                                event: InEvent::DeclineRegisterRequest {
                                    error: ErrorCode::InvalidTtl,
                                },
                            },
                            NetworkBehaviourAction::GenerateEvent(Event::DeclinedRegisterRequest {
                                peer: peer_id,
                            }),
                        ]
                    }
                };

                self.events.extend(events);
            }
            Message::RegisterResponse { ttl } => {
                self.events
                    .push_back(NetworkBehaviourAction::GenerateEvent(Event::Registered {
                        rendezvous_node: peer_id,
                        ttl,
                    }))
            }
            Message::FailedToRegister { error } => self.events.push_back(
                NetworkBehaviourAction::GenerateEvent(Event::FailedToRegister {
                    rendezvous_node: peer_id,
                    err_code: error,
                }),
            ),
            Message::Unregister { namespace } => {
                self.registrations.remove(namespace, peer_id);
                // TODO: Should send unregister response?
            }
            Message::Discover { namespace, cookie } => {
                let (registrations, cookie) = self
                    .registrations
                    .get(namespace, cookie)
                    .expect("TODO: error handling: send back bad cookie");

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

pub struct Registrations {
    registrations_for_peer: HashMap<(PeerId, String), RegistrationId>,
    registrations: HashMap<RegistrationId, Registration>,
    cookies: HashMap<Cookie, Vec<RegistrationId>>,
    ttl_upper_bound: i64,
}

#[derive(Debug, thiserror::Error)]
#[error("Requested TTL {requested}s is longer than what we allow ({upper_bound}s)")]
pub struct TtlTooLong {
    upper_bound: i64,
    requested: i64,
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

    pub fn add(&mut self, new_registration: NewRegistration) -> Result<i64, TtlTooLong> {
        let ttl = new_registration.effective_ttl();
        if ttl > self.ttl_upper_bound {
            return Err(TtlTooLong {
                upper_bound: self.ttl_upper_bound,
                requested: ttl,
            });
        }

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
    ) -> Result<(impl Iterator<Item = &Registration> + '_, Cookie), CookieNamespaceMismatch> {
        let cookie_namespace = cookie.as_ref().and_then(|cookie| cookie.namespace());

        match (discover_namespace.as_ref(), cookie_namespace) {
            // discover all namespace but cookie is specific to a namespace? => bad
            (None, Some(_)) => return Err(CookieNamespaceMismatch),
            // discover for a namespace but cookie is for a different namesapce? => bad
            (Some(namespace), Some(cookie_namespace)) if namespace != cookie_namespace => {
                return Err(CookieNamespaceMismatch)
            }
            // every other combination is fine
            _ => {}
        }

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

        let new_cookie = discover_namespace
            .map(Cookie::for_namespace)
            .unwrap_or_else(|| Cookie::for_all_namespaces());
        self.cookies
            .insert(new_cookie.clone(), reggos_of_last_discover);

        let reggos = &self.registrations;
        let registrations = ids
            .into_iter()
            .map(move |id| reggos.get(&id).expect("bad internal datastructure"));

        Ok((registrations, new_cookie))
    }
}

// TODO: Be more specific in what the bad combination was?
#[derive(Debug, thiserror::Error, Eq, PartialEq)]
#[error("The provided cookie is not valid for a DISCOVER request for the given namespace")]
pub struct CookieNamespaceMismatch;

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::identity;

    #[test]
    fn given_cookie_from_discover_when_discover_again_then_only_get_diff() {
        let mut registrations = Registrations::new(7200);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("foo")).unwrap();

        let (initial_discover, cookie) = registrations.get(None, None).unwrap();
        assert_eq!(initial_discover.collect::<Vec<_>>().len(), 2);

        let (subsequent_discover, _) = registrations.get(None, Some(cookie)).unwrap();
        assert_eq!(subsequent_discover.collect::<Vec<_>>().len(), 0);
    }

    #[test]
    fn given_registrations_when_discover_all_then_all_are_returned() {
        let mut registrations = Registrations::new(7200);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("foo")).unwrap();

        let (discover, _) = registrations.get(None, None).unwrap();

        assert_eq!(discover.collect::<Vec<_>>().len(), 2);
    }

    #[test]
    fn given_registrations_when_discover_only_for_specific_namespace_then_only_those_are_returned()
    {
        let mut registrations = Registrations::new(7200);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("bar")).unwrap();

        let (discover, _) = registrations.get(Some("foo".to_owned()), None).unwrap();

        assert_eq!(
            discover.map(|r| r.namespace.as_str()).collect::<Vec<_>>(),
            vec!["foo"]
        );
    }

    #[test]
    fn given_reregistration_old_registration_is_discarded() {
        let alice = identity::Keypair::generate_ed25519();
        let mut registrations = Registrations::new(7200);
        registrations
            .add(new_registration("foo", alice.clone()))
            .unwrap();
        registrations.add(new_registration("foo", alice)).unwrap();

        let (discover, _) = registrations.get(Some("foo".to_owned()), None).unwrap();

        assert_eq!(
            discover.map(|r| r.namespace.as_str()).collect::<Vec<_>>(),
            vec!["foo"]
        );
    }

    #[test]
    fn given_cookie_from_2nd_discover_does_not_return_nodes_from_first_discover() {
        let mut registrations = Registrations::new(7200);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("foo")).unwrap();

        let (initial_discover, cookie1) = registrations.get(None, None).unwrap();
        assert_eq!(initial_discover.collect::<Vec<_>>().len(), 2);

        let (subsequent_discover, cookie2) = registrations.get(None, Some(cookie1)).unwrap();
        assert_eq!(subsequent_discover.collect::<Vec<_>>().len(), 0);

        let (subsequent_discover, _) = registrations.get(None, Some(cookie2)).unwrap();
        assert_eq!(subsequent_discover.collect::<Vec<_>>().len(), 0);
    }

    #[test]
    fn cookie_from_different_discover_request_is_not_valid() {
        let mut registrations = Registrations::new(7200);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("bar")).unwrap();

        let (_, foo_discover_cookie) = registrations.get(Some("foo".to_owned()), None).unwrap();
        let result = registrations.get(Some("bar".to_owned()), Some(foo_discover_cookie));

        assert!(matches!(result, Err(CookieNamespaceMismatch)))
    }

    fn new_dummy_registration(namespace: &str) -> NewRegistration {
        let identity = identity::Keypair::generate_ed25519();

        new_registration(namespace, identity)
    }

    fn new_registration(namespace: &str, identity: identity::Keypair) -> NewRegistration {
        NewRegistration::new(
            namespace.to_owned(),
            PeerRecord::new(identity, vec!["/ip4/127.0.0.1/tcp/1234".parse().unwrap()]).unwrap(),
            None,
        )
    }
}
