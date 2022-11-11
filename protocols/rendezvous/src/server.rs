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
    Cookie, Error, ErrorCode, Message, Namespace, NewRegistration, Registration, RendezvousCodec,
    Ttl,
};
use crate::{MAX_TTL, MIN_TTL, PROTOCOL_IDENT};
use asynchronous_codec::Framed;
use bimap::BiMap;
use futures::future::BoxFuture;
use futures::stream::FuturesUnordered;
use futures::{ready, SinkExt, Stream};
use futures::{FutureExt, StreamExt};
use libp2p_core::connection::ConnectionId;
use libp2p_core::{ConnectedPoint, Multiaddr, PeerId};
use libp2p_swarm::handler::from_fn;
use libp2p_swarm::{
    from_fn, CloseConnection, IntoConnectionHandler, NegotiatedSubstream, NetworkBehaviour,
    NetworkBehaviourAction, PollParameters,
};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::io;
use std::iter::FromIterator;
use std::task::{Context, Poll};
use std::time::Duration;
use void::Void;

pub struct Behaviour {
    registrations: Registrations,
    registration_data: from_fn::Shared<RegistrationData>,
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
            registrations: Registrations::with_config(config),
            registration_data: from_fn::Shared::new(RegistrationData::default()),
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
        from_fn::FromFnProto<Result<Option<InboundOutEvent>, Error>, Void, Void, RegistrationData>;
    type OutEvent = Event;

    fn new_handler(&mut self) -> Self::ConnectionHandler {
        from_fn(PROTOCOL_IDENT)
            .with_state(&self.registration_data)
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
        self.registration_data
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
        self.registration_data
            .unregister_connection(*peer_id, *connection_id)
    }

    fn inject_event(
        &mut self,
        peer_id: PeerId,
        _: ConnectionId,
        event: from_fn::OutEvent<Result<Option<InboundOutEvent>, Error>, Void, Void>,
    ) {
        match event {
            from_fn::OutEvent::InboundEmitted(Ok(Some(InboundOutEvent::NewRegistration(
                new_registration,
            )))) => self
                .registrations
                .add(new_registration, &mut self.registration_data),
            from_fn::OutEvent::InboundEmitted(Ok(Some(InboundOutEvent::Unregister(
                namespace,
            )))) => self
                .registrations
                .remove(namespace, peer_id, &mut self.registration_data),
            from_fn::OutEvent::OutboundEmitted(_) => {}
            from_fn::OutEvent::FailedToOpen(never) => match never {
                from_fn::OpenError::Timeout(never) => void::unreachable(never),
                from_fn::OpenError::LimitExceeded(never) => void::unreachable(never),
                from_fn::OpenError::NegotiationFailed(never, _) => void::unreachable(never),
            },
            from_fn::OutEvent::InboundEmitted(Err(error)) => {
                log::debug!("Inbound stream from {peer_id} failed: {error}");
            }
            from_fn::OutEvent::InboundEmitted(Ok(None)) => {}
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        _: &mut impl PollParameters,
    ) -> Poll<NetworkBehaviourAction<Self::OutEvent, Self::ConnectionHandler>> {
        if let Poll::Ready(ExpiredRegistration(registration)) =
            self.registrations.poll(&mut self.registration_data, cx)
        {
            return Poll::Ready(NetworkBehaviourAction::GenerateEvent(
                Event::RegistrationExpired(registration),
            ));
        }

        if let Poll::Ready(action) = self.registration_data.poll(cx) {
            return Poll::Ready(action);
        }

        Poll::Pending
    }
}

fn inbound_stream_handler(
    substream: NegotiatedSubstream,
    _: PeerId,
    _: &ConnectedPoint,
    registrations: &RegistrationData,
) -> impl Future<Output = Result<Option<InboundOutEvent>, Error>> {
    let mut registrations = registrations.clone();
    let mut substream = Framed::new(substream, RendezvousCodec::default());

    async move {
        let message = substream
            .next()
            .await
            .ok_or_else(|| Error::Io(io::ErrorKind::UnexpectedEof.into()))??;

        let out_event = match message {
            Message::Register(new_registration) => {
                // TODO: Validate registration

                substream
                    .send(Message::RegisterResponse(Ok(
                        new_registration.effective_ttl()
                    )))
                    .await?;
                substream.close().await?;

                Some(InboundOutEvent::NewRegistration(new_registration))
            }
            Message::Unregister(namespace) => {
                substream.close().await?;

                Some(InboundOutEvent::Unregister(namespace))
            }
            Message::Discover {
                namespace,
                cookie,
                limit,
            } => match registrations.get(namespace, cookie, limit) {
                Ok((registrations, cookie)) => {
                    substream
                        .send(Message::DiscoverResponse(Ok((
                            registrations.cloned().collect(),
                            cookie,
                        ))))
                        .await?;
                    substream.close().await?;

                    None
                }
                Err(e) => {
                    substream
                        .send(Message::DiscoverResponse(Err(ErrorCode::InvalidCookie)))
                        .await?;
                    substream.close().await?;

                    None
                }
            },
            Message::DiscoverResponse(_) | Message::RegisterResponse(_) => {
                panic!("protocol violation")
            }
        };

        Ok(out_event)
    }
}

#[derive(Debug)]
pub enum InboundOutEvent {
    NewRegistration(NewRegistration),
    Unregister(Namespace),
}

#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
struct RegistrationId(u64);

impl RegistrationId {
    fn new() -> Self {
        Self(rand::random())
    }
}

#[derive(Debug, PartialEq)]
struct ExpiredRegistration(Registration);

pub struct Registrations {
    min_ttl: Ttl,
    max_ttl: Ttl,
    next_expiry: FuturesUnordered<BoxFuture<'static, RegistrationId>>,
}

#[derive(Debug, Clone, Default)]
pub struct RegistrationData {
    registrations_for_peer: BiMap<(PeerId, Namespace), RegistrationId>,
    registrations: HashMap<RegistrationId, Registration>,
    cookies: HashMap<Cookie, HashSet<RegistrationId>>,
}

impl RegistrationData {
    pub fn get(
        &mut self,
        discover_namespace: Option<Namespace>,
        cookie: Option<Cookie>,
        limit: Option<u64>,
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
            .take(limit.unwrap_or(u64::MAX) as usize)
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
}

#[derive(Debug, thiserror::Error)]
pub enum TtlOutOfRange {
    #[error("Requested TTL ({requested}s) is too long; max {bound}s")]
    TooLong { bound: Ttl, requested: Ttl },
    #[error("Requested TTL ({requested}s) is too short; min {bound}s")]
    TooShort { bound: Ttl, requested: Ttl },
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
            next_expiry: FuturesUnordered::from_iter(vec![futures::future::pending().boxed()]),
        }
    }

    pub fn add(&mut self, new_registration: NewRegistration, data: &mut RegistrationData) {
        // TOOD: Assume validation has by done by newtype.
        let ttl = new_registration.effective_ttl();
        // if ttl > self.max_ttl {
        //     return Err(TtlOutOfRange::TooLong {
        //         bound: self.max_ttl,
        //         requested: ttl,
        //     });
        // }
        // if ttl < self.min_ttl {
        //     return Err(TtlOutOfRange::TooShort {
        //         bound: self.min_ttl,
        //         requested: ttl,
        //     });
        // }

        let namespace = new_registration.namespace;
        let registration_id = RegistrationId::new();

        if let Some(old_registration) = data
            .registrations_for_peer
            .get_by_left(&(new_registration.record.peer_id(), namespace.clone()))
        {
            data.registrations.remove(old_registration);
        }

        data.registrations_for_peer.insert(
            (new_registration.record.peer_id(), namespace.clone()),
            registration_id,
        );

        let registration = Registration {
            namespace,
            record: new_registration.record,
            ttl,
        };
        data.registrations.insert(registration_id, registration);

        let next_expiry = futures_timer::Delay::new(Duration::from_secs(ttl as u64))
            .map(move |_| registration_id)
            .boxed();

        self.next_expiry.push(next_expiry);
    }

    pub fn remove(&self, namespace: Namespace, peer_id: PeerId, data: &mut RegistrationData) {
        let reggo_to_remove = data
            .registrations_for_peer
            .remove_by_left(&(peer_id, namespace));

        if let Some((_, reggo_to_remove)) = reggo_to_remove {
            data.registrations.remove(&reggo_to_remove);
        }
    }

    fn poll(
        &mut self,
        data: &mut RegistrationData,
        cx: &mut Context<'_>,
    ) -> Poll<ExpiredRegistration> {
        let expired_registration = ready!(self.next_expiry.poll_next_unpin(cx)).expect(
            "This stream should never finish because it is initialised with a pending future",
        );

        // clean up our cookies
        data.cookies.retain(|_, registrations| {
            registrations.remove(&expired_registration);

            // retain all cookies where there are still registrations left
            !registrations.is_empty()
        });

        data.registrations_for_peer
            .remove_by_right(&expired_registration);
        match data.registrations.remove(&expired_registration) {
            None => self.poll(data, cx),
            Some(registration) => Poll::Ready(ExpiredRegistration(registration)),
        }
    }
}

#[derive(Debug, thiserror::Error, Eq, PartialEq)]
#[error("The provided cookie is not valid for a DISCOVER request for the given namespace")]
pub struct CookieNamespaceMismatch;

#[cfg(test)]
mod tests {
    // use instant::SystemTime;
    // use std::option::Option::None;
    //
    // use libp2p_core::{identity, PeerRecord};
    //
    // use super::*;
    //
    // #[test]
    // fn given_cookie_from_discover_when_discover_again_then_only_get_diff() {
    //     let mut registrations = Registrations::default();
    //     registrations.add(new_dummy_registration("foo")).unwrap();
    //     registrations.add(new_dummy_registration("foo")).unwrap();
    //
    //     let (initial_discover, cookie) = registrations.get(None, None, None).unwrap();
    //     assert_eq!(initial_discover.count(), 2);
    //
    //     let (subsequent_discover, _) = registrations.get(None, Some(cookie), None).unwrap();
    //     assert_eq!(subsequent_discover.count(), 0);
    // }
    //
    // #[test]
    // fn given_registrations_when_discover_all_then_all_are_returned() {
    //     let mut registrations = Registrations::default();
    //     registrations.add(new_dummy_registration("foo")).unwrap();
    //     registrations.add(new_dummy_registration("foo")).unwrap();
    //
    //     let (discover, _) = registrations.get(None, None, None).unwrap();
    //
    //     assert_eq!(discover.count(), 2);
    // }
    //
    // #[test]
    // fn given_registrations_when_discover_only_for_specific_namespace_then_only_those_are_returned()
    // {
    //     let mut registrations = Registrations::default();
    //     registrations.add(new_dummy_registration("foo")).unwrap();
    //     registrations.add(new_dummy_registration("bar")).unwrap();
    //
    //     let (discover, _) = registrations
    //         .get(Some(Namespace::from_static("foo")), None, None)
    //         .unwrap();
    //
    //     assert_eq!(
    //         discover.map(|r| &r.namespace).collect::<Vec<_>>(),
    //         vec!["foo"]
    //     );
    // }
    //
    // #[test]
    // fn given_reregistration_old_registration_is_discarded() {
    //     let alice = identity::Keypair::generate_ed25519();
    //     let mut registrations = Registrations::default();
    //     registrations
    //         .add(new_registration("foo", alice.clone(), None))
    //         .unwrap();
    //     registrations
    //         .add(new_registration("foo", alice, None))
    //         .unwrap();
    //
    //     let (discover, _) = registrations
    //         .get(Some(Namespace::from_static("foo")), None, None)
    //         .unwrap();
    //
    //     assert_eq!(
    //         discover.map(|r| &r.namespace).collect::<Vec<_>>(),
    //         vec!["foo"]
    //     );
    // }
    //
    // #[test]
    // fn given_cookie_from_2nd_discover_does_not_return_nodes_from_first_discover() {
    //     let mut registrations = Registrations::default();
    //     registrations.add(new_dummy_registration("foo")).unwrap();
    //     registrations.add(new_dummy_registration("foo")).unwrap();
    //
    //     let (initial_discover, cookie1) = registrations.get(None, None, None).unwrap();
    //     assert_eq!(initial_discover.count(), 2);
    //
    //     let (subsequent_discover, cookie2) = registrations.get(None, Some(cookie1), None).unwrap();
    //     assert_eq!(subsequent_discover.count(), 0);
    //
    //     let (subsequent_discover, _) = registrations.get(None, Some(cookie2), None).unwrap();
    //     assert_eq!(subsequent_discover.count(), 0);
    // }
    //
    // #[test]
    // fn cookie_from_different_discover_request_is_not_valid() {
    //     let mut registrations = Registrations::default();
    //     registrations.add(new_dummy_registration("foo")).unwrap();
    //     registrations.add(new_dummy_registration("bar")).unwrap();
    //
    //     let (_, foo_discover_cookie) = registrations
    //         .get(Some(Namespace::from_static("foo")), None, None)
    //         .unwrap();
    //     let result = registrations.get(
    //         Some(Namespace::from_static("bar")),
    //         Some(foo_discover_cookie),
    //         None,
    //     );
    //
    //     assert!(matches!(result, Err(CookieNamespaceMismatch)))
    // }
    //
    // #[tokio::test]
    // async fn given_two_registration_ttls_one_expires_one_lives() {
    //     let mut registrations = Registrations::with_config(Config {
    //         min_ttl: 0,
    //         max_ttl: 4,
    //     });
    //
    //     let start_time = SystemTime::now();
    //
    //     registrations
    //         .add(new_dummy_registration_with_ttl("foo", 1))
    //         .unwrap();
    //     registrations
    //         .add(new_dummy_registration_with_ttl("bar", 4))
    //         .unwrap();
    //
    //     let event = registrations.next_event().await;
    //
    //     let elapsed = start_time.elapsed().unwrap();
    //     assert!(elapsed.as_secs() >= 1);
    //     assert!(elapsed.as_secs() < 2);
    //
    //     assert_eq!(event.0.namespace, Namespace::from_static("foo"));
    //
    //     {
    //         let (mut discovered_foo, _) = registrations
    //             .get(Some(Namespace::from_static("foo")), None, None)
    //             .unwrap();
    //         assert!(discovered_foo.next().is_none());
    //     }
    //     let (mut discovered_bar, _) = registrations
    //         .get(Some(Namespace::from_static("bar")), None, None)
    //         .unwrap();
    //     assert!(discovered_bar.next().is_some());
    // }
    //
    // #[tokio::test]
    // async fn given_peer_unregisters_before_expiry_do_not_emit_registration_expired() {
    //     let mut registrations = Registrations::with_config(Config {
    //         min_ttl: 1,
    //         max_ttl: 10,
    //     });
    //     let dummy_registration = new_dummy_registration_with_ttl("foo", 2);
    //     let namespace = dummy_registration.namespace.clone();
    //     let peer_id = dummy_registration.record.peer_id();
    //
    //     registrations.add(dummy_registration).unwrap();
    //     registrations.no_event_for(1).await;
    //     registrations.remove(namespace, peer_id);
    //
    //     registrations.no_event_for(3).await
    // }
    //
    // /// FuturesUnordered stop polling for ready futures when poll_next() is called until a None
    // /// value is returned. To prevent the next_expiry future from going to "sleep", next_expiry
    // /// is initialised with a future that always returns pending. This test ensures that
    // /// FuturesUnordered does not stop polling for ready futures.
    // #[tokio::test]
    // async fn given_all_registrations_expired_then_successfully_handle_new_registration_and_expiry()
    // {
    //     let mut registrations = Registrations::with_config(Config {
    //         min_ttl: 0,
    //         max_ttl: 10,
    //     });
    //     let dummy_registration = new_dummy_registration_with_ttl("foo", 1);
    //
    //     registrations.add(dummy_registration.clone()).unwrap();
    //     let _ = registrations.next_event_in_at_most(2).await;
    //
    //     registrations.no_event_for(1).await;
    //
    //     registrations.add(dummy_registration).unwrap();
    //     let _ = registrations.next_event_in_at_most(2).await;
    // }
    //
    // #[tokio::test]
    // async fn cookies_are_cleaned_up_if_registrations_expire() {
    //     let mut registrations = Registrations::with_config(Config {
    //         min_ttl: 1,
    //         max_ttl: 10,
    //     });
    //
    //     registrations
    //         .add(new_dummy_registration_with_ttl("foo", 2))
    //         .unwrap();
    //     let (_, _) = registrations.get(None, None, None).unwrap();
    //
    //     assert_eq!(registrations.data.cookies.len(), 1);
    //
    //     let _ = registrations.next_event_in_at_most(3).await;
    //
    //     assert_eq!(registrations.data.cookies.len(), 0);
    // }
    //
    // #[test]
    // fn given_limit_discover_only_returns_n_results() {
    //     let mut registrations = Registrations::default();
    //     registrations.add(new_dummy_registration("foo")).unwrap();
    //     registrations.add(new_dummy_registration("foo")).unwrap();
    //
    //     let (registrations, _) = registrations.get(None, None, Some(1)).unwrap();
    //
    //     assert_eq!(registrations.count(), 1);
    // }
    //
    // #[test]
    // fn given_limit_cookie_can_be_used_for_pagination() {
    //     let mut registrations = Registrations::default();
    //     registrations.add(new_dummy_registration("foo")).unwrap();
    //     registrations.add(new_dummy_registration("foo")).unwrap();
    //
    //     let (discover1, cookie) = registrations.get(None, None, Some(1)).unwrap();
    //     assert_eq!(discover1.count(), 1);
    //
    //     let (discover2, _) = registrations.get(None, Some(cookie), None).unwrap();
    //     assert_eq!(discover2.count(), 1);
    // }
    //
    // fn new_dummy_registration(namespace: &'static str) -> NewRegistration {
    //     let identity = identity::Keypair::generate_ed25519();
    //
    //     new_registration(namespace, identity, None)
    // }
    //
    // fn new_dummy_registration_with_ttl(namespace: &'static str, ttl: Ttl) -> NewRegistration {
    //     let identity = identity::Keypair::generate_ed25519();
    //
    //     new_registration(namespace, identity, Some(ttl))
    // }
    //
    // fn new_registration(
    //     namespace: &'static str,
    //     identity: identity::Keypair,
    //     ttl: Option<Ttl>,
    // ) -> NewRegistration {
    //     NewRegistration::new(
    //         Namespace::from_static(namespace),
    //         PeerRecord::new(&identity, vec!["/ip4/127.0.0.1/tcp/1234".parse().unwrap()]).unwrap(),
    //         ttl,
    //     )
    // }
    //
    // /// Defines utility functions that make the tests more readable.
    // impl Registrations {
    //     async fn next_event(&mut self) -> ExpiredRegistration {
    //         futures::future::poll_fn(|cx| self.poll(cx)).await
    //     }
    //
    //     /// Polls [`Registrations`] for `seconds` and panics if it returns a event during this time.
    //     async fn no_event_for(&mut self, seconds: u64) {
    //         tokio::time::timeout(Duration::from_secs(seconds), self.next_event())
    //             .await
    //             .unwrap_err();
    //     }
    //
    //     /// Polls [`Registrations`] for at most `seconds` and panics if doesn't return an event within that time.
    //     async fn next_event_in_at_most(&mut self, seconds: u64) -> ExpiredRegistration {
    //         tokio::time::timeout(Duration::from_secs(seconds), self.next_event())
    //             .await
    //             .unwrap()
    //     }
    // }
}
