pub use crate::pow::Difficulty;

use crate::codec::{Cookie, ErrorCode, NewRegistration, Registration};
use crate::handler;
use crate::handler::{DeclineReason, InEvent, OutEvent, RendezvousHandler};
use bimap::BiMap;
use futures::future::BoxFuture;
use futures::ready;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use libp2p_core::connection::ConnectionId;
use libp2p_core::identity::error::SigningError;
use libp2p_core::identity::Keypair;
use libp2p_core::{Multiaddr, PeerId, PeerRecord};
use libp2p_swarm::{
    NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters, ProtocolsHandler,
};
use log::debug;
use std::collections::{HashMap, HashSet, VecDeque};
use std::iter::FromIterator;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime};
use tokio::time::sleep;
use uuid::Uuid;

pub struct Rendezvous {
    events: VecDeque<NetworkBehaviourAction<InEvent, Event>>,
    registrations: Registrations,
    key_pair: Keypair,
    external_addresses: Vec<Multiaddr>,

    /// The maximum PoW difficulty we are willing to accept from a rendezvous point.
    max_accepted_difficulty: Difficulty,
}

impl Rendezvous {
    pub fn new(
        key_pair: Keypair,
        ttl_upper_board: i64,
        max_accepted_difficulty: Difficulty,
    ) -> Self {
        Self {
            events: Default::default(),
            registrations: Registrations::new(ttl_upper_board),
            key_pair,
            external_addresses: vec![],
            max_accepted_difficulty,
        }
    }

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
    DiscoverFailed {
        rendezvous_node: PeerId,
        namespace: Option<String>,
        error: ErrorCode,
    },
    /// We successfully registered with the contained rendezvous node.
    Registered {
        rendezvous_node: PeerId,
        ttl: i64,
        namespace: String,
    },
    /// We failed to register with the contained rendezvous node.
    RegisterFailed {
        rendezvous_node: PeerId,
        namespace: String,
        error: ErrorCode,
    },
    /// We successfully served a discover request from a peer.
    DiscoverServed {
        enquirer: PeerId,
        registrations: Vec<Registration>,
    },
    /// A peer successfully registered with us.
    PeerRegistered {
        peer: PeerId,
        registration: Registration,
    },
    /// We declined a registration from a peer.
    PeerNotRegistered {
        peer: PeerId,
        namespace: String,
        error: ErrorCode,
    },
    /// A peer successfully unregistered with us.
    PeerUnregistered { peer: PeerId, namespace: String },
}

impl NetworkBehaviour for Rendezvous {
    type ProtocolsHandler = RendezvousHandler;
    type OutEvent = Event;

    fn new_handler(&mut self) -> Self::ProtocolsHandler {
        RendezvousHandler::new(self.max_accepted_difficulty)
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
        connection: ConnectionId,
        event: handler::OutEvent,
    ) {
        match event {
            OutEvent::RegistrationRequested {
                registration,
                pow_difficulty: provided_difficulty,
            } => {
                let expected_difficulty = self.registrations.expected_pow(&registration);

                if expected_difficulty > provided_difficulty {
                    self.events
                        .push_back(NetworkBehaviourAction::NotifyHandler {
                            peer_id,
                            handler: NotifyHandler::One(connection),
                            event: InEvent::DeclineRegisterRequest(DeclineReason::PowRequired {
                                target: expected_difficulty,
                            }),
                        });
                    return;
                }

                let namespace = registration.namespace.clone();
                let record = registration.record.clone();

                let events = match self.registrations.add(registration) {
                    Ok((effective_ttl, timestamp)) => {
                        vec![
                            NetworkBehaviourAction::NotifyHandler {
                                peer_id,
                                handler: NotifyHandler::One(connection),
                                event: InEvent::RegisterResponse { ttl: effective_ttl },
                            },
                            NetworkBehaviourAction::GenerateEvent(Event::PeerRegistered {
                                peer: peer_id,
                                registration: Registration {
                                    namespace,
                                    record,
                                    ttl: effective_ttl,
                                    timestamp,
                                },
                            }),
                        ]
                    }
                    Err(TtlTooLong { .. }) => {
                        let error = ErrorCode::InvalidTtl;

                        vec![
                            NetworkBehaviourAction::NotifyHandler {
                                peer_id,
                                handler: NotifyHandler::One(connection),
                                event: InEvent::DeclineRegisterRequest(
                                    DeclineReason::BadRegistration(error),
                                ),
                            },
                            NetworkBehaviourAction::GenerateEvent(Event::PeerNotRegistered {
                                peer: peer_id,
                                namespace,
                                error,
                            }),
                        ]
                    }
                };

                self.events.extend(events);
            }
            OutEvent::Registered { namespace, ttl } => {
                self.events
                    .push_back(NetworkBehaviourAction::GenerateEvent(Event::Registered {
                        rendezvous_node: peer_id,
                        ttl,
                        namespace,
                    }))
            }
            OutEvent::RegisterFailed { namespace, error } => self.events.push_back(
                NetworkBehaviourAction::GenerateEvent(Event::RegisterFailed {
                    rendezvous_node: peer_id,
                    namespace,
                    error,
                }),
            ),
            OutEvent::UnregisterRequested { namespace } => {
                self.registrations.remove(namespace, peer_id);
            }
            OutEvent::DiscoverRequested { namespace, cookie } => {
                let (registrations, cookie) = self
                    .registrations
                    .get(namespace, cookie)
                    .expect("TODO: error handling: send back bad cookie");

                let discovered = registrations.cloned().collect::<Vec<_>>();

                self.events
                    .push_back(NetworkBehaviourAction::NotifyHandler {
                        peer_id,
                        handler: NotifyHandler::One(connection),
                        event: InEvent::DiscoverResponse {
                            discovered: discovered.clone(),
                            cookie,
                        },
                    });
                self.events.push_back(NetworkBehaviourAction::GenerateEvent(
                    Event::DiscoverServed {
                        enquirer: peer_id,
                        registrations: discovered,
                    },
                ));
            }
            OutEvent::Discovered {
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
            OutEvent::DiscoverFailed { namespace, error } => self.events.push_back(
                NetworkBehaviourAction::GenerateEvent(Event::DiscoverFailed {
                    rendezvous_node: peer_id,
                    namespace,
                    error,
                }),
            ),
        }
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
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

#[derive(Debug, PartialEq)]
pub struct RegistrationExpired(Registration);

pub struct Registrations {
    registrations_for_peer: BiMap<(PeerId, String), RegistrationId>,
    registrations: HashMap<RegistrationId, Registration>,
    cookies: HashMap<Cookie, HashSet<RegistrationId>>,
    ttl_upper_bound: i64,
    next_expiry: FuturesUnordered<BoxFuture<'static, RegistrationId>>,
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
            next_expiry: FuturesUnordered::from_iter(vec![futures::future::pending().boxed()]),
        }
    }

    pub fn add(
        &mut self,
        new_registration: NewRegistration,
    ) -> Result<(i64, SystemTime), TtlTooLong> {
        let ttl = new_registration.effective_ttl();
        if ttl > self.ttl_upper_bound {
            return Err(TtlTooLong {
                upper_bound: self.ttl_upper_bound,
                requested: ttl,
            });
        }

        let namespace = new_registration.namespace;
        let registration_id = RegistrationId::new();

        if let Some(old_registration) = self
            .registrations_for_peer
            .get_by_left(&(new_registration.record.peer_id(), namespace.clone()))
        {
            self.registrations.remove(old_registration);
        }

        self.registrations_for_peer.insert(
            (new_registration.record.peer_id(), namespace.clone()),
            registration_id,
        );

        let timestamp = SystemTime::now();

        self.registrations.insert(
            registration_id,
            Registration {
                namespace,
                record: new_registration.record,
                ttl,
                timestamp,
            },
        );

        let next_expiry = sleep(Duration::from_secs(ttl as u64))
            .map(move |()| registration_id)
            .boxed();

        self.next_expiry.push(next_expiry);

        Ok((ttl, timestamp))
    }

    pub fn remove(&mut self, namespace: String, peer_id: PeerId) {
        let reggo_to_remove = self
            .registrations_for_peer
            .remove_by_left(&(peer_id, namespace));

        if let Some((_, reggo_to_remove)) = reggo_to_remove {
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

        reggos_of_last_discover.extend(&ids);

        let new_cookie = discover_namespace
            .map(Cookie::for_namespace)
            .unwrap_or_else(Cookie::for_all_namespaces);
        self.cookies
            .insert(new_cookie.clone(), reggos_of_last_discover);

        let reggos = &self.registrations;
        let registrations = ids
            .into_iter()
            .map(move |id| reggos.get(&id).expect("bad internal datastructure"));

        Ok((registrations, new_cookie))
    }

    pub fn expected_pow(&self, registration: &NewRegistration) -> Difficulty {
        let peer = registration.record.peer_id();

        let num_registrations = self
            .registrations_for_peer
            .left_values()
            .filter(|(candidate, _)| candidate == &peer)
            .count();

        difficulty_from_num_registrations(num_registrations)
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<RegistrationExpired> {
        let expired_registration = ready!(self.next_expiry.poll_next_unpin(cx)).expect(
            "This stream should never finish because it is initialised with a pending future",
        );

        // clean up our cookies
        self.cookies.retain(|_, registrations| {
            registrations.remove(&expired_registration);

            // retain all cookies where there are still registrations left
            !registrations.is_empty()
        });

        self.registrations_for_peer
            .remove_by_right(&expired_registration);
        match self.registrations.remove(&expired_registration) {
            None => self.poll(cx),
            Some(registration) => Poll::Ready(RegistrationExpired(registration)),
        }
    }
}

fn difficulty_from_num_registrations(existing_registrations: usize) -> Difficulty {
    if existing_registrations == 0 {
        return Difficulty::ZERO;
    }

    let new_registrations = existing_registrations + 1;

    Difficulty::from_u32(((new_registrations) as f32 / 2f32).round() as u32)
        .unwrap_or(Difficulty::MAX)
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
#[error("The provided cookie is not valid for a DISCOVER request for the given namespace")]
pub struct CookieNamespaceMismatch;

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p_core::identity;

    #[tokio::test]
    async fn given_cookie_from_discover_when_discover_again_then_only_get_diff() {
        let mut registrations = Registrations::new(7200);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("foo")).unwrap();

        let (initial_discover, cookie) = registrations.get(None, None).unwrap();
        assert_eq!(initial_discover.collect::<Vec<_>>().len(), 2);

        let (subsequent_discover, _) = registrations.get(None, Some(cookie)).unwrap();
        assert_eq!(subsequent_discover.collect::<Vec<_>>().len(), 0);
    }

    #[tokio::test]
    async fn given_registrations_when_discover_all_then_all_are_returned() {
        let mut registrations = Registrations::new(7200);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("foo")).unwrap();

        let (discover, _) = registrations.get(None, None).unwrap();

        assert_eq!(discover.collect::<Vec<_>>().len(), 2);
    }

    #[tokio::test]
    async fn given_registrations_when_discover_only_for_specific_namespace_then_only_those_are_returned(
    ) {
        let mut registrations = Registrations::new(7200);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("bar")).unwrap();

        let (discover, _) = registrations.get(Some("foo".to_owned()), None).unwrap();

        assert_eq!(
            discover.map(|r| r.namespace.as_str()).collect::<Vec<_>>(),
            vec!["foo"]
        );
    }

    #[tokio::test]
    async fn given_reregistration_old_registration_is_discarded() {
        let alice = identity::Keypair::generate_ed25519();
        let mut registrations = Registrations::new(7200);
        registrations
            .add(new_registration("foo", alice.clone(), None))
            .unwrap();
        registrations
            .add(new_registration("foo", alice, None))
            .unwrap();

        let (discover, _) = registrations.get(Some("foo".to_owned()), None).unwrap();

        assert_eq!(
            discover.map(|r| r.namespace.as_str()).collect::<Vec<_>>(),
            vec!["foo"]
        );
    }

    #[tokio::test]
    async fn given_cookie_from_2nd_discover_does_not_return_nodes_from_first_discover() {
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

    #[tokio::test]
    async fn cookie_from_different_discover_request_is_not_valid() {
        let mut registrations = Registrations::new(7200);
        registrations.add(new_dummy_registration("foo")).unwrap();
        registrations.add(new_dummy_registration("bar")).unwrap();

        let (_, foo_discover_cookie) = registrations.get(Some("foo".to_owned()), None).unwrap();
        let result = registrations.get(Some("bar".to_owned()), Some(foo_discover_cookie));

        assert!(matches!(result, Err(CookieNamespaceMismatch)))
    }

    #[tokio::test]
    async fn given_two_registration_ttls_one_expires_one_lives() {
        let mut registrations = Registrations::new(7200);

        let start_time = SystemTime::now();

        registrations
            .add(new_dummy_registration_with_ttl("foo", 1))
            .unwrap();
        registrations
            .add(new_dummy_registration_with_ttl("bar", 4))
            .unwrap();

        let event = registrations.next_event().await;

        let elapsed = start_time.elapsed().unwrap();
        assert!(elapsed.as_secs() >= 1);
        assert!(elapsed.as_secs() < 2);

        assert_eq!(event.0.namespace, "foo");

        {
            let (mut discovered_foo, _) = registrations.get(Some("foo".to_owned()), None).unwrap();
            assert!(discovered_foo.next().is_none());
        }
        let (mut discovered_bar, _) = registrations.get(Some("bar".to_owned()), None).unwrap();
        assert!(discovered_bar.next().is_some());
    }

    #[tokio::test]
    async fn given_peer_unregisters_before_expiry_do_not_emit_registration_expired() {
        let mut registrations = Registrations::new(7200);
        let dummy_registration = new_dummy_registration_with_ttl("foo", 2);
        let namespace = dummy_registration.namespace.clone();
        let peer_id = dummy_registration.record.peer_id();

        registrations.add(dummy_registration).unwrap();
        registrations.no_event_for(1).await;
        registrations.remove(namespace, peer_id);

        registrations.no_event_for(3).await
    }

    /// FuturesUnordered stop polling for ready futures when poll_next() is called until a None
    /// value is returned. To prevent the next_expiry future from going to "sleep", next_expiry
    /// is initialised with a future that always returns pending. This test ensures that
    /// FuturesUnordered does not stop polling for ready futures.
    #[tokio::test]
    async fn given_all_registrations_expired_then_succesfully_handle_new_registration_and_expiry() {
        let mut registrations = Registrations::new(7200);
        let dummy_registration = new_dummy_registration_with_ttl("foo", 1);

        registrations.add(dummy_registration.clone()).unwrap();
        let _ = registrations.next_event_in_at_most(2).await;

        registrations.no_event_for(1).await;

        registrations.add(dummy_registration).unwrap();
        let _ = registrations.next_event_in_at_most(2).await;
    }

    #[tokio::test]
    async fn cookies_are_cleaned_up_if_registrations_expire() {
        let mut registrations = Registrations::new(7200);

        registrations
            .add(new_dummy_registration_with_ttl("foo", 2))
            .unwrap();
        let (_, cookie) = registrations.get(None, None).unwrap();

        assert_eq!(registrations.cookies.len(), 1);

        let _ = registrations.next_event_in_at_most(3).await;

        assert_eq!(registrations.cookies.len(), 0);
    }

    #[test]
    fn first_registration_is_free() {
        let required = difficulty_from_num_registrations(0);
        let expected = Difficulty::ZERO;

        assert_eq!(required, expected)
    }

    #[test]
    fn second_registration_is_not_free() {
        let required = difficulty_from_num_registrations(1);
        let expected = Difficulty::from_u32(1).unwrap();

        assert_eq!(required, expected)
    }

    #[test]
    fn fourth_registration_requires_two() {
        let required = difficulty_from_num_registrations(3);
        let expected = Difficulty::from_u32(2).unwrap();

        assert_eq!(required, expected)
    }

    fn new_dummy_registration(namespace: &str) -> NewRegistration {
        let identity = identity::Keypair::generate_ed25519();

        new_registration(namespace, identity, None)
    }

    fn new_dummy_registration_with_ttl(namespace: &str, ttl: i64) -> NewRegistration {
        let identity = identity::Keypair::generate_ed25519();

        new_registration(namespace, identity, Some(ttl))
    }

    fn new_registration(
        namespace: &str,
        identity: identity::Keypair,
        ttl: Option<i64>,
    ) -> NewRegistration {
        NewRegistration::new(
            namespace.to_owned(),
            PeerRecord::new(identity, vec!["/ip4/127.0.0.1/tcp/1234".parse().unwrap()]).unwrap(),
            ttl,
        )
    }

    /// Defines utility functions that make the tests more readable.
    impl Registrations {
        async fn next_event(&mut self) -> RegistrationExpired {
            futures::future::poll_fn(|cx| self.poll(cx)).await
        }

        /// Polls [`Registrations`] for `seconds` and panics if it returns a event during this time.
        async fn no_event_for(&mut self, seconds: u64) {
            tokio::time::timeout(Duration::from_secs(seconds), self.next_event())
                .await
                .unwrap_err();
        }

        /// Polls [`Registrations`] for at most `seconds` and panics if doesn't return an event within that time.
        async fn next_event_in_at_most(&mut self, seconds: u64) -> RegistrationExpired {
            tokio::time::timeout(Duration::from_secs(seconds), self.next_event())
                .await
                .unwrap()
        }
    }
}
