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

use crate::codec::{
    Cookie, Error, ErrorCode, Message, Namespace, NewRegistrationRequest, Registration,
    RendezvousCodec, Ttl,
};
use crate::{DEFAULT_TTL, MAX_TTL, MIN_TTL, PROTOCOL_IDENT};
use asynchronous_codec::Framed;
use bimap::BiMap;
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::{ready, SinkExt};
use futures::{FutureExt, StreamExt};
use libp2p_core::connection::ConnectionId;
use libp2p_core::{ConnectedPoint, Multiaddr, PeerId, PeerRecord};
use libp2p_swarm::handler::from_fn;
use libp2p_swarm::{
    from_fn, IntoConnectionHandler, NegotiatedSubstream, NetworkBehaviour, NetworkBehaviourAction,
    PollParameters,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use void::Void;

pub struct Behaviour {
    next_expiry: FuturesUnordered<BoxFuture<'static, RegistrationId>>,
    registrations: from_fn::Shared<Registrations>,
    events: VecDeque<
        NetworkBehaviourAction<
            Event,
            from_fn::FromFnProto<Result<Option<InboundOutEvent>, Error>, Void, Void, Registrations>,
        >,
    >,
}

pub struct Config {
    min_ttl: Ttl,
    max_ttl: Ttl,
}

impl Config {
    pub fn with_min_ttl(mut self, min_ttl: Ttl) -> Self {
        self.min_ttl = min_ttl;
        self
    }

    pub fn with_max_ttl(mut self, max_ttl: Ttl) -> Self {
        self.max_ttl = max_ttl;
        self
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            min_ttl: MIN_TTL,
            max_ttl: MAX_TTL,
        }
    }
}

impl Behaviour {
    /// Create a new instance of the rendezvous [`NetworkBehaviour`].
    pub fn new(config: Config) -> Self {
        Self {
            next_expiry: FuturesUnordered::new(),
            registrations: from_fn::Shared::new(Registrations::with_config(config)),
            events: Default::default(),
        }
    }

    fn poll_expiry_timers(&mut self, cx: &mut Context<'_>) -> Poll<Option<ExpiredRegistration>> {
        loop {
            let expired_registration = match ready!(self.next_expiry.poll_next_unpin(cx)) {
                Some(r) => r,
                None => return Poll::Ready(None), // TODO: Register waker and return Pending?
            };

            // clean up our cookies
            self.registrations.cookies.retain(|_, registrations| {
                registrations.remove(&expired_registration);

                // retain all cookies where there are still registrations left
                !registrations.is_empty()
            });

            self.registrations
                .registrations_for_peer
                .remove_by_right(&expired_registration);

            match self
                .registrations
                .registrations
                .remove(&expired_registration)
            {
                Some(registration) => return Poll::Ready(Some(ExpiredRegistration(registration))),
                None => continue,
            }
        }
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Event {
    /// We successfully served a discover request from a peer.
    DiscoverServed {
        enquirer: PeerId,
        registrations: Vec<Registration>,
    },
    /// We failed to serve a discover request for a peer.
    DiscoverNotServed { enquirer: PeerId, error: ErrorCode },
    /// A peer successfully registered with us.
    PeerRegistered {
        peer: PeerId,
        registration: Registration,
    },
    /// We declined a registration from a peer.
    PeerNotRegistered {
        peer: PeerId,
        namespace: Namespace,
        error: ErrorCode,
    },
    /// A peer successfully unregistered with us.
    PeerUnregistered { peer: PeerId, namespace: Namespace },
    /// A registration from a peer expired.
    RegistrationExpired(Registration),
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler =
        from_fn::FromFnProto<Result<Option<InboundOutEvent>, Error>, Void, Void, Registrations>;
    type OutEvent = Event;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        from_fn(PROTOCOL_IDENT)
            .with_state(&self.registrations)
            .with_inbound_handler(10, inbound_stream_handler)
            .without_outbound_handler()
    }

    fn inject_connection_established(
        &mut self,
        peer_id: &PeerId,
        connection_id: &ConnectionId,
        _: &ConnectedPoint,
        _: Option<&Vec<Multiaddr>>,
        _: usize,
    ) {
        self.registrations
            .register_connection(*peer_id, *connection_id)
    }

    fn inject_connection_closed(
        &mut self,
        peer_id: &PeerId,
        connection_id: &ConnectionId,
        _: &ConnectedPoint,
        _: <Self::ConnectionHandler as IntoConnectionHandler>::Handler,
        _remaining_established: usize,
    ) {
        self.registrations
            .unregister_connection(*peer_id, *connection_id)
    }

    fn inject_event(
        &mut self,
        peer: PeerId,
        _: ConnectionId,
        event: from_fn::OutEvent<Result<Option<InboundOutEvent>, Error>, Void, Void>,
    ) {
        let new_events = match event {
            from_fn::OutEvent::InboundEmitted(Ok(Some(InboundOutEvent::NewRegistration(
                new_registration,
            )))) => {
                let (registration, expiry) = self.registrations.add(new_registration);
                self.next_expiry.push(expiry);

                vec![NetworkBehaviourAction::GenerateEvent(
                    Event::PeerRegistered { peer, registration },
                )]
            }
            from_fn::OutEvent::InboundEmitted(Ok(Some(InboundOutEvent::RegistrationFailed {
                error,
                namespace,
            }))) => {
                vec![NetworkBehaviourAction::GenerateEvent(
                    Event::PeerNotRegistered {
                        peer,
                        error,
                        namespace,
                    },
                )]
            }
            from_fn::OutEvent::InboundEmitted(Ok(Some(InboundOutEvent::Discovered {
                cookie,
                registrations,
                previous_registrations,
            }))) => {
                self.registrations
                    .cookies
                    .insert(cookie, previous_registrations);

                vec![NetworkBehaviourAction::GenerateEvent(
                    Event::DiscoverServed {
                        enquirer: peer,
                        registrations,
                    },
                )]
            }
            from_fn::OutEvent::InboundEmitted(Ok(Some(InboundOutEvent::DiscoverFailed {
                error,
            }))) => {
                vec![NetworkBehaviourAction::GenerateEvent(
                    Event::DiscoverNotServed {
                        enquirer: peer,
                        error,
                    },
                )]
            }
            from_fn::OutEvent::InboundEmitted(Ok(Some(InboundOutEvent::Unregister(namespace)))) => {
                self.registrations.remove(namespace.clone(), peer);

                vec![NetworkBehaviourAction::GenerateEvent(
                    Event::PeerUnregistered { peer, namespace },
                )]
            }
            from_fn::OutEvent::OutboundEmitted(never) => void::unreachable(never),
            from_fn::OutEvent::FailedToOpen(never) => match never {
                from_fn::OpenError::Timeout(never) => void::unreachable(never),
                from_fn::OpenError::LimitExceeded {
                    open_info: never, ..
                } => void::unreachable(never),
                from_fn::OpenError::NegotiationFailed(never, _) => void::unreachable(never),
                from_fn::OpenError::Unsupported {
                    open_info: never, ..
                } => void::unreachable(never),
            },
            from_fn::OutEvent::InboundEmitted(Err(error)) => {
                log::debug!("Inbound stream from {peer} failed: {error}");

                vec![]
            }
            from_fn::OutEvent::InboundEmitted(Ok(None)) => {
                vec![]
            }
        };

        self.events.extend(new_events)
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        if let Some(event) = self.events.pop_front() {
            return Poll::Ready(event);
        }

        if let Poll::Ready(Some(ExpiredRegistration(registration))) = self.poll_expiry_timers(cx) {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                Event::RegistrationExpired(registration),
            ));
        }

        if let Poll::Ready(action) = self.registrations.poll(cx) {
            return Poll::Ready(action);
        }

        Poll::Pending
    }
}

async fn inbound_stream_handler(
    substream: NegotiatedSubstream,
    peer: PeerId,
    _: ConnectedPoint,
    registrations: Arc<Registrations>,
) -> Result<Option<InboundOutEvent>, Error> {
    let mut substream = Framed::new(substream, RendezvousCodec::default());

    let message = substream
        .next()
        .await
        .ok_or_else(|| Error::Io(io::ErrorKind::UnexpectedEof.into()))??;

    let out_event = match message {
        Message::Register(new_registration) => {
            let namespace = new_registration.namespace.clone();

            match registrations.new_registration(peer, new_registration) {
                Ok(new_registration) => {
                    substream
                        .send(Message::RegisterResponse(Ok(
                            new_registration.effective_ttl()
                        )))
                        .await?;

                    Some(InboundOutEvent::NewRegistration(new_registration))
                }
                Err(error) => {
                    substream
                        .send(Message::RegisterResponse(Err(error)))
                        .await?;

                    Some(InboundOutEvent::RegistrationFailed { namespace, error })
                }
            }
        }
        Message::Unregister(namespace) => Some(InboundOutEvent::Unregister(namespace)),
        Message::Discover {
            namespace,
            cookie,
            limit,
        } => match registrations.get(namespace, cookie, limit) {
            Ok((registrations, cookie, sent_registrations)) => {
                let registrations = registrations.cloned().collect::<Vec<_>>();

                substream
                    .send(Message::DiscoverResponse(Ok((
                        registrations.clone(),
                        cookie.clone(),
                    ))))
                    .await?;

                Some(InboundOutEvent::Discovered {
                    cookie,
                    registrations,
                    previous_registrations: sent_registrations,
                })
            }
            Err(_) => {
                substream
                    .send(Message::DiscoverResponse(Err(ErrorCode::InvalidCookie)))
                    .await?;

                Some(InboundOutEvent::DiscoverFailed {
                    error: ErrorCode::InvalidCookie,
                })
            }
        },
        Message::DiscoverResponse(_) | Message::RegisterResponse(_) => {
            panic!("protocol violation")
        }
    };

    substream.close().await?;

    Ok(out_event)
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum InboundOutEvent {
    NewRegistration(NewRegistration),
    RegistrationFailed {
        namespace: Namespace,
        error: ErrorCode,
    },
    Discovered {
        cookie: Cookie,
        registrations: Vec<Registration>,
        previous_registrations: HashSet<RegistrationId>,
    },
    DiscoverFailed {
        error: ErrorCode,
    },
    Unregister(Namespace),
}

#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
pub struct RegistrationId(u64);

impl RegistrationId {
    fn new() -> Self {
        Self(rand::random())
    }
}

#[derive(Debug, PartialEq)]
struct ExpiredRegistration(Registration);

#[derive(Debug, Clone)]
pub struct Registrations {
    min_ttl: Ttl,
    max_ttl: Ttl,

    registrations_for_peer: BiMap<(PeerId, Namespace), RegistrationId>,
    registrations: HashMap<RegistrationId, Registration>,
    cookies: HashMap<Cookie, HashSet<RegistrationId>>,
}

#[derive(Debug)]
pub struct NewRegistration {
    namespace: Namespace,
    record: PeerRecord,
    ttl: Option<u64>,
}

impl NewRegistration {
    pub fn effective_ttl(&self) -> Ttl {
        self.ttl.unwrap_or(DEFAULT_TTL)
    }
}

impl Default for Registrations {
    fn default() -> Self {
        Registrations::with_config(Config::default())
    }
}

impl Registrations {
    pub fn with_config(config: Config) -> Self {
        Self {
            min_ttl: config.min_ttl,
            max_ttl: config.max_ttl,
            registrations_for_peer: Default::default(),
            registrations: Default::default(),
            cookies: Default::default(),
        }
    }

    pub fn new_registration(
        &self,
        from: PeerId,
        new_registration: NewRegistrationRequest,
    ) -> Result<NewRegistration, ErrorCode> {
        let ttl = new_registration.ttl.unwrap_or(DEFAULT_TTL);

        if ttl > self.max_ttl {
            return Err(ErrorCode::InvalidTtl);
        }
        if ttl < self.min_ttl {
            return Err(ErrorCode::InvalidTtl);
        }

        if new_registration.record.peer_id() != from {
            return Err(ErrorCode::NotAuthorized);
        }

        Ok(NewRegistration {
            namespace: new_registration.namespace,
            record: new_registration.record,
            ttl: new_registration.ttl,
        })
    }

    pub fn add(
        &mut self,
        new_registration: NewRegistration,
    ) -> (Registration, BoxFuture<'static, RegistrationId>) {
        let ttl = new_registration.effective_ttl();
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

        let registration = Registration {
            namespace,
            record: new_registration.record,
            ttl,
        };
        self.registrations
            .insert(registration_id, registration.clone());

        let expiry = futures_timer::Delay::new(Duration::from_secs(ttl as u64))
            .map(move |_| registration_id)
            .boxed();

        (registration, expiry)
    }

    pub fn remove(&mut self, namespace: Namespace, peer_id: PeerId) {
        let reggo_to_remove = self
            .registrations_for_peer
            .remove_by_left(&(peer_id, namespace));

        if let Some((_, reggo_to_remove)) = reggo_to_remove {
            self.registrations.remove(&reggo_to_remove);
        }
    }

    pub fn get(
        &self,
        discover_namespace: Option<Namespace>,
        cookie: Option<Cookie>,
        limit: Option<u64>,
    ) -> Result<
        (
            impl Iterator<Item = &Registration> + '_,
            Cookie,
            HashSet<RegistrationId>,
        ),
        CookieNamespaceMismatch,
    > {
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
            .take(limit.unwrap_or(u64::MAX) as usize)
            .cloned()
            .collect::<Vec<_>>();

        reggos_of_last_discover.extend(&ids);

        let new_cookie = discover_namespace
            .map(Cookie::for_namespace)
            .unwrap_or_else(Cookie::for_all_namespaces);

        let reggos = &self.registrations;
        let registrations = ids
            .into_iter()
            .map(move |id| reggos.get(&id).expect("bad internal datastructure"));

        Ok((registrations, new_cookie, reggos_of_last_discover))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TtlOutOfRange {
    #[error("Requested TTL ({requested}s) is too long; max {bound}s")]
    TooLong { bound: Ttl, requested: Ttl },
    #[error("Requested TTL ({requested}s) is too short; min {bound}s")]
    TooShort { bound: Ttl, requested: Ttl },
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
#[error("The provided cookie is not valid for a DISCOVER request for the given namespace")]
pub struct CookieNamespaceMismatch;

#[cfg(test)]
mod tests {
    use libp2p_core::{identity, PeerRecord};

    use super::*;

    #[test]
    fn given_cookie_from_discover_when_discover_again_then_only_get_diff() {
        let mut registrations = Registrations::default();
        registrations.add(new_dummy_registration("foo"));
        registrations.add(new_dummy_registration("foo"));

        let (initial_discover, cookie, existing_registrations) =
            registrations.get(None, None, None).unwrap();
        assert_eq!(initial_discover.count(), 2);
        registrations
            .cookies
            .insert(cookie.clone(), existing_registrations);

        let (subsequent_discover, _, _) = registrations.get(None, Some(cookie), None).unwrap();
        assert_eq!(subsequent_discover.count(), 0);
    }

    #[test]
    fn given_registrations_when_discover_all_then_all_are_returned() {
        let mut registrations = Registrations::default();
        registrations.add(new_dummy_registration("foo"));
        registrations.add(new_dummy_registration("foo"));

        let (discover, _, _) = registrations.get(None, None, None).unwrap();

        assert_eq!(discover.count(), 2);
    }

    #[test]
    fn given_registrations_when_discover_only_for_specific_namespace_then_only_those_are_returned()
    {
        let mut registrations = Registrations::default();
        registrations.add(new_dummy_registration("foo"));
        registrations.add(new_dummy_registration("bar"));

        let (discover, _, _) = registrations
            .get(Some(Namespace::from_static("foo")), None, None)
            .unwrap();

        assert_eq!(
            discover.map(|r| &r.namespace).collect::<Vec<_>>(),
            vec!["foo"]
        );
    }

    #[test]
    fn given_reregistration_old_registration_is_discarded() {
        let alice = identity::Keypair::generate_ed25519();
        let mut registrations = Registrations::default();
        registrations.add(new_registration("foo", alice.clone(), None));
        registrations.add(new_registration("foo", alice, None));

        let (discover, _, _) = registrations
            .get(Some(Namespace::from_static("foo")), None, None)
            .unwrap();

        assert_eq!(
            discover.map(|r| &r.namespace).collect::<Vec<_>>(),
            vec!["foo"]
        );
    }

    #[test]
    fn given_cookie_from_2nd_discover_does_not_return_nodes_from_first_discover() {
        let mut registrations = Registrations::default();
        registrations.add(new_dummy_registration("foo"));
        registrations.add(new_dummy_registration("foo"));

        let (initial_discover, cookie1, existing_registrations) =
            registrations.get(None, None, None).unwrap();
        assert_eq!(initial_discover.count(), 2);
        registrations
            .cookies
            .insert(cookie1.clone(), existing_registrations);

        let (subsequent_discover, cookie2, existing_registrations) =
            registrations.get(None, Some(cookie1), None).unwrap();
        assert_eq!(subsequent_discover.count(), 0);
        registrations
            .cookies
            .insert(cookie2.clone(), existing_registrations);

        let (subsequent_discover, _, _) = registrations.get(None, Some(cookie2), None).unwrap();
        assert_eq!(subsequent_discover.count(), 0);
    }

    #[test]
    fn cookie_from_different_discover_request_is_not_valid() {
        let mut registrations = Registrations::default();
        registrations.add(new_dummy_registration("foo"));
        registrations.add(new_dummy_registration("bar"));

        let (_, foo_discover_cookie, _) = registrations
            .get(Some(Namespace::from_static("foo")), None, None)
            .unwrap();
        let result = registrations.get(
            Some(Namespace::from_static("bar")),
            Some(foo_discover_cookie),
            None,
        );

        assert!(matches!(result, Err(CookieNamespaceMismatch)))
    }

    #[test]
    fn given_limit_discover_only_returns_n_results() {
        let mut registrations = Registrations::default();
        registrations.add(new_dummy_registration("foo"));
        registrations.add(new_dummy_registration("foo"));

        let (registrations, _, _) = registrations.get(None, None, Some(1)).unwrap();

        assert_eq!(registrations.count(), 1);
    }

    #[test]
    fn given_limit_cookie_can_be_used_for_pagination() {
        let mut registrations = Registrations::default();
        registrations.add(new_dummy_registration("foo"));
        registrations.add(new_dummy_registration("foo"));

        let (discover1, cookie, existing_registrations) =
            registrations.get(None, None, Some(1)).unwrap();
        assert_eq!(discover1.count(), 1);
        registrations
            .cookies
            .insert(cookie.clone(), existing_registrations);

        let (discover2, _, _) = registrations.get(None, Some(cookie), None).unwrap();
        assert_eq!(discover2.count(), 1);
    }

    fn new_dummy_registration(namespace: &'static str) -> NewRegistration {
        let identity = identity::Keypair::generate_ed25519();

        new_registration(namespace, identity, None)
    }

    fn new_registration(
        namespace: &'static str,
        identity: identity::Keypair,
        ttl: Option<Ttl>,
    ) -> NewRegistration {
        NewRegistration {
            namespace: Namespace::from_static(namespace),
            record: PeerRecord::new(&identity, vec!["/ip4/127.0.0.1/tcp/1234".parse().unwrap()])
                .unwrap(),
            ttl,
        }
    }
}
