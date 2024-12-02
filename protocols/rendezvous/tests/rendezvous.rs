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

use std::time::Duration;

use futures::{stream::FuturesUnordered, StreamExt};
use libp2p_core::{multiaddr::Protocol, Multiaddr};
use libp2p_identity as identity;
use libp2p_rendezvous as rendezvous;
use libp2p_rendezvous::client::RegisterError;
use libp2p_swarm::{DialError, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use tracing_subscriber::EnvFilter;

#[tokio::test]
async fn given_successful_registration_then_successful_discovery() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice, mut bob], mut robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default()).await;

    alice
        .behaviour_mut()
        .register(namespace.clone(), *robert.local_peer_id(), None)
        .unwrap();

    match libp2p_swarm_test::drive(&mut alice, &mut robert).await {
        (
            [rendezvous::client::Event::Registered {
                rendezvous_node,
                ttl,
                namespace: register_node_namespace,
            }],
            [rendezvous::server::Event::PeerRegistered { peer, registration }],
        ) => {
            assert_eq!(&peer, alice.local_peer_id());
            assert_eq!(&rendezvous_node, robert.local_peer_id());
            assert_eq!(registration.namespace, namespace);
            assert_eq!(register_node_namespace, namespace);
            assert_eq!(ttl, rendezvous::DEFAULT_TTL);
        }
        events => panic!("Unexpected events: {events:?}"),
    }

    bob.behaviour_mut()
        .discover(Some(namespace.clone()), None, None, *robert.local_peer_id());

    match libp2p_swarm_test::drive(&mut bob, &mut robert).await {
        (
            [rendezvous::client::Event::Discovered { registrations, .. }],
            [rendezvous::server::Event::DiscoverServed { .. }],
        ) => match registrations.as_slice() {
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
        },
        events => panic!("Unexpected events: {events:?}"),
    }
}

#[tokio::test]
async fn should_return_error_when_no_external_addresses() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let server = new_server(rendezvous::server::Config::default()).await;
    let mut client = Swarm::new_ephemeral(rendezvous::client::Behaviour::new);

    let actual = client
        .behaviour_mut()
        .register(namespace.clone(), *server.local_peer_id(), None)
        .unwrap_err();

    assert!(matches!(actual, RegisterError::NoExternalAddresses))
}

#[tokio::test]
async fn given_successful_registration_then_refresh_ttl() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice, mut bob], mut robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default()).await;

    let roberts_peer_id = *robert.local_peer_id();
    let refresh_ttl = 10_000;

    alice
        .behaviour_mut()
        .register(namespace.clone(), roberts_peer_id, None)
        .unwrap();

    match libp2p_swarm_test::drive(&mut alice, &mut robert).await {
        (
            [rendezvous::client::Event::Registered { .. }],
            [rendezvous::server::Event::PeerRegistered { .. }],
        ) => {}
        events => panic!("Unexpected events: {events:?}"),
    }

    bob.behaviour_mut()
        .discover(Some(namespace.clone()), None, None, roberts_peer_id);

    match libp2p_swarm_test::drive(&mut bob, &mut robert).await {
        (
            [rendezvous::client::Event::Discovered { .. }],
            [rendezvous::server::Event::DiscoverServed { .. }],
        ) => {}
        events => panic!("Unexpected events: {events:?}"),
    }

    alice
        .behaviour_mut()
        .register(namespace.clone(), roberts_peer_id, Some(refresh_ttl))
        .unwrap();

    match libp2p_swarm_test::drive(&mut alice, &mut robert).await {
        (
            [rendezvous::client::Event::Registered { ttl, .. }],
            [rendezvous::server::Event::PeerRegistered { .. }],
        ) => {
            assert_eq!(ttl, refresh_ttl);
        }
        events => panic!("Unexpected events: {events:?}"),
    }

    bob.behaviour_mut()
        .discover(Some(namespace.clone()), None, None, *robert.local_peer_id());

    match libp2p_swarm_test::drive(&mut bob, &mut robert).await {
        (
            [rendezvous::client::Event::Discovered { registrations, .. }],
            [rendezvous::server::Event::DiscoverServed { .. }],
        ) => match registrations.as_slice() {
            [rendezvous::Registration { ttl, .. }] => {
                assert_eq!(*ttl, refresh_ttl);
            }
            _ => panic!("Expected exactly one registration to be returned from discover"),
        },
        events => panic!("Unexpected events: {events:?}"),
    }
}

#[tokio::test]
async fn given_successful_registration_then_refresh_external_addrs() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice], mut robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default()).await;

    let roberts_peer_id = *robert.local_peer_id();

    alice
        .behaviour_mut()
        .register(namespace.clone(), roberts_peer_id, None)
        .unwrap();

    match libp2p_swarm_test::drive(&mut alice, &mut robert).await {
        (
            [rendezvous::client::Event::Registered { .. }],
            [rendezvous::server::Event::PeerRegistered { .. }],
        ) => {}
        events => panic!("Unexpected events: {events:?}"),
    }

    let external_addr = Multiaddr::empty().with(Protocol::Memory(0));

    alice.add_external_address(external_addr.clone());

    match libp2p_swarm_test::drive(&mut alice, &mut robert).await {
        (
            [rendezvous::client::Event::Registered { .. }],
            [rendezvous::server::Event::PeerRegistered { registration, .. }],
        ) => {
            let record = registration.record;
            assert!(record.addresses().contains(&external_addr));
        }
        events => panic!("Unexpected events: {events:?}"),
    }

    alice.remove_external_address(&external_addr);

    match libp2p_swarm_test::drive(&mut alice, &mut robert).await {
        (
            [rendezvous::client::Event::Registered { .. }],
            [rendezvous::server::Event::PeerRegistered { registration, .. }],
        ) => {
            let record = registration.record;
            assert!(!record.addresses().contains(&external_addr));
        }
        events => panic!("Unexpected events: {events:?}"),
    }
}

#[tokio::test]
async fn given_invalid_ttl_then_unsuccessful_registration() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice], mut robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default()).await;

    alice
        .behaviour_mut()
        .register(
            namespace.clone(),
            *robert.local_peer_id(),
            Some(100_000_000),
        )
        .unwrap();

    match libp2p_swarm_test::drive(&mut alice, &mut robert).await {
        (
            [rendezvous::client::Event::RegisterFailed { error, .. }],
            [rendezvous::server::Event::PeerNotRegistered { .. }],
        ) => {
            assert_eq!(error, rendezvous::ErrorCode::InvalidTtl);
        }
        events => panic!("Unexpected events: {events:?}"),
    }
}

#[tokio::test]
async fn discover_allows_for_dial_by_peer_id() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice, mut bob], robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default()).await;

    let roberts_peer_id = *robert.local_peer_id();
    tokio::spawn(robert.loop_on_next());

    alice
        .behaviour_mut()
        .register(namespace.clone(), roberts_peer_id, None)
        .unwrap();
    match alice.next_behaviour_event().await {
        rendezvous::client::Event::Registered { .. } => {}
        event => panic!("Unexpected event: {event:?}"),
    }

    bob.behaviour_mut()
        .discover(Some(namespace.clone()), None, None, roberts_peer_id);
    match bob.next_behaviour_event().await {
        rendezvous::client::Event::Discovered { registrations, .. } => {
            assert!(!registrations.is_empty());
        }
        event => panic!("Unexpected event: {event:?}"),
    }

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
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let mut robert = new_server(rendezvous::server::Config::default()).await;
    let mut eve = new_impersonating_client().await;
    eve.connect(&mut robert).await;

    eve.behaviour_mut()
        .register(namespace.clone(), *robert.local_peer_id(), None)
        .unwrap();

    match libp2p_swarm_test::drive(&mut eve, &mut robert).await {
        (
            [rendezvous::client::Event::RegisterFailed {
                error: err_code, ..
            }],
            [rendezvous::server::Event::PeerNotRegistered { .. }],
        ) => {
            assert_eq!(err_code, rendezvous::ErrorCode::NotAuthorized);
        }
        events => panic!("Unexpected events: {events:?}"),
    }
}

// test if charlie can operate as client and server simultaneously
#[tokio::test]
async fn can_combine_client_and_server() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice], mut robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default()).await;
    let mut charlie = new_combined_node().await;
    charlie.connect(&mut robert).await;
    alice.connect(&mut charlie).await;

    charlie
        .behaviour_mut()
        .client
        .register(namespace.clone(), *robert.local_peer_id(), None)
        .unwrap();
    match libp2p_swarm_test::drive(&mut charlie, &mut robert).await {
        (
            [CombinedEvent::Client(rendezvous::client::Event::Registered { .. })],
            [rendezvous::server::Event::PeerRegistered { .. }],
        ) => {}
        events => panic!("Unexpected events: {events:?}"),
    }

    alice
        .behaviour_mut()
        .register(namespace, *charlie.local_peer_id(), None)
        .unwrap();
    match libp2p_swarm_test::drive(&mut charlie, &mut alice).await {
        (
            [CombinedEvent::Server(rendezvous::server::Event::PeerRegistered { .. })],
            [rendezvous::client::Event::Registered { .. }],
        ) => {}
        events => panic!("Unexpected events: {events:?}"),
    }
}

#[tokio::test]
async fn registration_on_clients_expire() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let namespace = rendezvous::Namespace::from_static("some-namespace");
    let ([mut alice, mut bob], robert) =
        new_server_with_connected_clients(rendezvous::server::Config::default().with_min_ttl(1))
            .await;

    let roberts_peer_id = *robert.local_peer_id();
    tokio::spawn(robert.loop_on_next());

    let registration_ttl = 1;

    alice
        .behaviour_mut()
        .register(namespace.clone(), roberts_peer_id, Some(registration_ttl))
        .unwrap();
    match alice.next_behaviour_event().await {
        rendezvous::client::Event::Registered { .. } => {}
        event => panic!("Unexpected event: {event:?}"),
    }
    bob.behaviour_mut()
        .discover(Some(namespace), None, None, roberts_peer_id);
    match bob.next_behaviour_event().await {
        rendezvous::client::Event::Discovered { registrations, .. } => {
            assert!(!registrations.is_empty());
        }
        event => panic!("Unexpected event: {event:?}"),
    }

    tokio::time::sleep(Duration::from_secs(registration_ttl + 1)).await;

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
        client.connect(&mut server).await;
    }

    (clients, server)
}

async fn new_client() -> Swarm<rendezvous::client::Behaviour> {
    let mut client = Swarm::new_ephemeral(rendezvous::client::Behaviour::new);
    client.listen().with_memory_addr_external().await; // we need to listen otherwise we don't have addresses to register

    client
}

async fn new_server(config: rendezvous::server::Config) -> Swarm<rendezvous::server::Behaviour> {
    let mut server = Swarm::new_ephemeral(|_| rendezvous::server::Behaviour::new(config));

    server.listen().with_memory_addr_external().await;

    server
}

async fn new_combined_node() -> Swarm<Combined> {
    let mut node = Swarm::new_ephemeral(|identity| Combined {
        client: rendezvous::client::Behaviour::new(identity),
        server: rendezvous::server::Behaviour::new(rendezvous::server::Config::default()),
    });
    node.listen().with_memory_addr_external().await;

    node
}

async fn new_impersonating_client() -> Swarm<rendezvous::client::Behaviour> {
    // In reality, if Eve were to try and fake someones identity, she would obviously only know the
    // public key. Due to the type-safe API of the `Rendezvous` behaviour and `PeerRecord`, we
    // actually cannot construct a bad `PeerRecord` (i.e. one that is claims to be someone else).
    // As such, the best we can do is hand eve a completely different keypair from what she is using
    // to authenticate her connection.
    let someone_else = identity::Keypair::generate_ed25519();
    let mut eve = Swarm::new_ephemeral(move |_| rendezvous::client::Behaviour::new(someone_else));
    eve.listen().with_memory_addr_external().await;

    eve
}

#[derive(libp2p_swarm::NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct Combined {
    client: rendezvous::client::Behaviour,
    server: rendezvous::server::Behaviour,
}
