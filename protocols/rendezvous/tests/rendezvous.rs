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

#[macro_use]
pub mod harness;

use crate::harness::{await_event_or_timeout, await_events_or_timeout, new_swarm, SwarmExt};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use libp2p::core::identity;
use libp2p::rendezvous;
use libp2p::swarm::{DialError, Swarm, SwarmEvent};
use std::convert::TryInto;
use std::time::Duration;

#[tokio::test]
async fn given_successful_registration_then_successful_discovery() {
    let _ = env_logger::try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice, mut bob], mut robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default()).await;

    alice
        .behaviour_mut()
        .register(namespace.clone(), *robert.local_peer_id(), None);

    assert_behaviour_events! {
        alice: rendezvous::client::Event::Registered { rendezvous_node, ttl, namespace: register_node_namespace },
        robert: rendezvous::server::Event::PeerRegistered { peer, registration },
        || {
            assert_eq!(&peer, alice.local_peer_id());
            assert_eq!(&rendezvous_node, robert.local_peer_id());
            assert_eq!(registration.namespace, namespace);
            assert_eq!(register_node_namespace, namespace);
            assert_eq!(ttl, rendezvous::DEFAULT_TTL);
        }
    };

    bob.behaviour_mut()
        .discover(Some(namespace.clone()), None, None, *robert.local_peer_id());

    assert_behaviour_events! {
        bob: rendezvous::client::Event::Discovered { registrations, .. },
        robert: rendezvous::server::Event::DiscoverServed { .. },
        || {
            match registrations.as_slice() {
                [rendezvous::Registration {
                    namespace: registered_namespace,
                    record,
                    ttl,
                }] => {
                    assert_eq!(*ttl, rendezvous::DEFAULT_TTL);
                    assert_eq!(record.peer_id(), *alice.local_peer_id());
                    assert_eq!(*registered_namespace, namespace);
                }
                _ => panic!("Expected exactly one registration to be returned from discover"),
            }
        }
    };
}

#[tokio::test]
async fn given_successful_registration_then_refresh_ttl() {
    let _ = env_logger::try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice, mut bob], mut robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default()).await;

    let roberts_peer_id = *robert.local_peer_id();
    let refresh_ttl = 10_000;

    alice
        .behaviour_mut()
        .register(namespace.clone(), roberts_peer_id, None);

    assert_behaviour_events! {
        alice: rendezvous::client::Event::Registered { .. },
        robert: rendezvous::server::Event::PeerRegistered { .. },
        || { }
    };

    bob.behaviour_mut()
        .discover(Some(namespace.clone()), None, None, roberts_peer_id);

    assert_behaviour_events! {
        bob: rendezvous::client::Event::Discovered { .. },
        robert: rendezvous::server::Event::DiscoverServed { .. },
        || { }
    };

    alice
        .behaviour_mut()
        .register(namespace.clone(), roberts_peer_id, Some(refresh_ttl));

    assert_behaviour_events! {
        alice: rendezvous::client::Event::Registered { ttl, .. },
        robert: rendezvous::server::Event::PeerRegistered { .. },
        || {
            assert_eq!(ttl, refresh_ttl);
        }
    };

    bob.behaviour_mut()
        .discover(Some(namespace.clone()), None, None, *robert.local_peer_id());

    assert_behaviour_events! {
        bob: rendezvous::client::Event::Discovered { registrations, .. },
        robert: rendezvous::server::Event::DiscoverServed { .. },
        || {
            match registrations.as_slice() {
                [rendezvous::Registration { ttl, .. }] => {
                    assert_eq!(*ttl, refresh_ttl);
                }
                _ => panic!("Expected exactly one registration to be returned from discover"),
            }
        }
    };
}

#[tokio::test]
async fn given_invalid_ttl_then_unsuccessful_registration() {
    let _ = env_logger::try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice], mut robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default()).await;

    alice.behaviour_mut().register(
        namespace.clone(),
        *robert.local_peer_id(),
        Some(100_000_000),
    );

    assert_behaviour_events! {
        alice: rendezvous::client::Event::RegisterFailed(rendezvous::client::RegisterError::Remote {error , ..}),
        robert: rendezvous::server::Event::PeerNotRegistered { .. },
        || {
            assert_eq!(error, rendezvous::ErrorCode::InvalidTtl);
        }
    };
}

#[tokio::test]
async fn discover_allows_for_dial_by_peer_id() {
    let _ = env_logger::try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice, mut bob], robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default()).await;

    let roberts_peer_id = *robert.local_peer_id();
    robert.spawn_into_runtime();

    alice
        .behaviour_mut()
        .register(namespace.clone(), roberts_peer_id, None);
    assert_behaviour_events! {
        alice: rendezvous::client::Event::Registered { .. },
        || { }
    };

    bob.behaviour_mut()
        .discover(Some(namespace.clone()), None, None, roberts_peer_id);
    assert_behaviour_events! {
        bob: rendezvous::client::Event::Discovered { registrations,.. },
        || {
            assert!(!registrations.is_empty());
        }
    };

    let alices_peer_id = *alice.local_peer_id();
    let bobs_peer_id = *bob.local_peer_id();

    bob.dial(alices_peer_id).unwrap();

    let alice_connected_to = tokio::spawn(async move {
        loop {
            if let SwarmEvent::ConnectionEstablished { peer_id, .. } =
                alice.select_next_some().await
            {
                break peer_id;
            }
        }
    });
    let bob_connected_to = tokio::spawn(async move {
        loop {
            if let SwarmEvent::ConnectionEstablished { peer_id, .. } = bob.select_next_some().await
            {
                break peer_id;
            }
        }
    });

    assert_eq!(alice_connected_to.await.unwrap(), bobs_peer_id);
    assert_eq!(bob_connected_to.await.unwrap(), alices_peer_id);
}

#[tokio::test]
async fn eve_cannot_register() {
    let _ = env_logger::try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let mut robert = new_server(rendezvous::server::Config::default()).await;
    let mut eve = new_impersonating_client().await;
    eve.block_on_connection(&mut robert).await;

    eve.behaviour_mut()
        .register(namespace.clone(), *robert.local_peer_id(), None);

    assert_behaviour_events! {
        eve: rendezvous::client::Event::RegisterFailed(rendezvous::client::RegisterError::Remote { error: err_code , ..}),
        robert: rendezvous::server::Event::PeerNotRegistered { .. },
        || {
            assert_eq!(err_code, rendezvous::ErrorCode::NotAuthorized);
        }
    };
}

// test if charlie can operate as client and server simultaneously
#[tokio::test]
async fn can_combine_client_and_server() {
    let _ = env_logger::try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice], mut robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default()).await;
    let mut charlie = new_combined_node().await;
    charlie.block_on_connection(&mut robert).await;
    alice.block_on_connection(&mut charlie).await;

    charlie
        .behaviour_mut()
        .client
        .register(namespace.clone(), *robert.local_peer_id(), None);

    assert_behaviour_events! {
        charlie: CombinedEvent::Client(rendezvous::client::Event::Registered { .. }),
        robert: rendezvous::server::Event::PeerRegistered { .. },
        || { }
    };

    alice
        .behaviour_mut()
        .register(namespace, *charlie.local_peer_id(), None);

    assert_behaviour_events! {
        charlie: CombinedEvent::Server(rendezvous::server::Event::PeerRegistered { .. }),
        alice: rendezvous::client::Event::Registered { .. },
        || { }
    };
}

#[tokio::test]
async fn registration_on_clients_expire() {
    let _ = env_logger::try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice, mut bob], robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default().with_min_ttl(1))
            .await;

    let roberts_peer_id = *robert.local_peer_id();
    robert.spawn_into_runtime();

    let registration_ttl = 3;

    alice
        .behaviour_mut()
        .register(namespace.clone(), roberts_peer_id, Some(registration_ttl));
    assert_behaviour_events! {
        alice: rendezvous::client::Event::Registered { .. },
        || { }
    };
    bob.behaviour_mut()
        .discover(Some(namespace), None, None, roberts_peer_id);
    assert_behaviour_events! {
        bob: rendezvous::client::Event::Discovered { registrations,.. },
        || {
            assert!(!registrations.is_empty());
        }
    };

    tokio::time::sleep(Duration::from_secs(registration_ttl + 5)).await;

    let event = bob.select_next_some().await;
    let error = bob.dial(*alice.local_peer_id()).unwrap_err();

    assert!(matches!(
        event,
        SwarmEvent::Behaviour(rendezvous::client::Event::Expired { .. })
    ));
    assert!(matches!(error, DialError::NoAddresses));
}

async fn new_server_with_connected_clients<const N: usize>(
    config: rendezvous::server::Config,
) -> (
    [Swarm<rendezvous::client::Behaviour>; N],
    Swarm<rendezvous::server::Behaviour>,
) {
    let mut server = new_server(config).await;

    let mut clients: [Swarm<_>; N] = match (0usize..N)
        .map(|_| new_client())
        .collect::<FuturesUnordered<_>>()
        .collect::<Vec<_>>()
        .await
        .try_into()
    {
        Ok(clients) => clients,
        Err(_) => panic!("Vec is of size N"),
    };

    for client in &mut clients {
        client.block_on_connection(&mut server).await;
    }

    (clients, server)
}

async fn new_client() -> Swarm<rendezvous::client::Behaviour> {
    let mut client = new_swarm(|_, identity| rendezvous::client::Behaviour::new(identity));
    client.listen_on_random_memory_address().await; // we need to listen otherwise we don't have addresses to register

    client
}

async fn new_server(config: rendezvous::server::Config) -> Swarm<rendezvous::server::Behaviour> {
    let mut server = new_swarm(|_, _| rendezvous::server::Behaviour::new(config));

    server.listen_on_random_memory_address().await;

    server
}

async fn new_combined_node() -> Swarm<CombinedBehaviour> {
    let mut node = new_swarm(|_, identity| CombinedBehaviour {
        client: rendezvous::client::Behaviour::new(identity),
        server: rendezvous::server::Behaviour::new(rendezvous::server::Config::default()),
    });
    node.listen_on_random_memory_address().await;

    node
}

async fn new_impersonating_client() -> Swarm<rendezvous::client::Behaviour> {
    // In reality, if Eve were to try and fake someones identity, she would obviously only know the public key.
    // Due to the type-safe API of the `Rendezvous` behaviour and `PeerRecord`, we actually cannot construct a bad `PeerRecord` (i.e. one that is claims to be someone else).
    // As such, the best we can do is hand eve a completely different keypair from what she is using to authenticate her connection.
    let someone_else = identity::Keypair::generate_ed25519();
    let mut eve = new_swarm(move |_, _| rendezvous::client::Behaviour::new(someone_else));
    eve.listen_on_random_memory_address().await;

    eve
}

#[derive(libp2p::NetworkBehaviour)]
#[behaviour(event_process = false, out_event = "CombinedEvent")]
struct CombinedBehaviour {
    client: rendezvous::client::Behaviour,
    server: rendezvous::server::Behaviour,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum CombinedEvent {
    Client(rendezvous::client::Event),
    Server(rendezvous::server::Event),
}

impl From<rendezvous::server::Event> for CombinedEvent {
    fn from(v: rendezvous::server::Event) -> Self {
        Self::Server(v)
    }
}

impl From<rendezvous::client::Event> for CombinedEvent {
    fn from(v: rendezvous::client::Event) -> Self {
        Self::Client(v)
    }
}
