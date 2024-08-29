// Copyright 2021 Protocol Labs.
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

use futures::executor::LocalPool;
use futures::future::FutureExt;
use futures::io::{AsyncRead, AsyncWrite};
use futures::stream::StreamExt;
use futures::task::Spawn;
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::transport::choice::OrTransport;
use libp2p_core::transport::{Boxed, MemoryTransport, Transport};
use libp2p_core::upgrade;
use libp2p_identity as identity;
use libp2p_identity::PeerId;
use libp2p_ping as ping;
use libp2p_plaintext as plaintext;
use libp2p_relay as relay;
use libp2p_swarm::dial_opts::DialOpts;
use libp2p_swarm::{Config, DialError, NetworkBehaviour, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use std::error::Error;
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[test]
fn reservation() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let mut relay = build_relay();
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone());
    spawn_swarm_on_pool(&pool, relay);

    let client_addr = relay_addr
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit);
    let mut client = build_client();

    client.listen_on(client_addr.clone()).unwrap();

    // Wait for connection to relay.
    assert!(pool.run_until(wait_for_dial(&mut client, relay_peer_id)));

    // Wait for initial reservation.
    pool.run_until(wait_for_reservation(
        &mut client,
        client_addr.clone(),
        relay_peer_id,
        false, // No renewal.
    ));

    // Wait for renewal.
    pool.run_until(wait_for_reservation(
        &mut client,
        client_addr,
        relay_peer_id,
        true, // Renewal.
    ));
}

#[test]
fn new_reservation_to_same_relay_replaces_old() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let mut relay = build_relay();
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone());
    spawn_swarm_on_pool(&pool, relay);

    let mut client = build_client();
    let client_addr = relay_addr
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit);

    let old_listener = client.listen_on(client_addr.clone()).unwrap();

    // Wait for connection to relay.
    assert!(pool.run_until(wait_for_dial(&mut client, relay_peer_id)));

    // Wait for first (old) reservation.
    pool.run_until(wait_for_reservation(
        &mut client,
        client_addr.clone(),
        relay_peer_id,
        false, // No renewal.
    ));

    // Trigger new reservation.
    let new_listener = client.listen_on(client_addr.clone()).unwrap();

    // Wait for
    // - listener of old reservation to close
    // - old external addr to expire
    // - new reservation to be accepted
    // - new listener address to be reported
    // - new external addr to be reported
    pool.run_until(async {
        let mut old_listener_closed = false;
        let mut old_external_addr_expired = false;
        let mut new_reservation_accepted = false;
        let mut new_listen_address_reported = false;
        let mut new_external_addr_confirmed = false;

        loop {
            match client.select_next_some().await {
                SwarmEvent::ListenerClosed {
                    addresses,
                    listener_id,
                    ..
                } => {
                    assert_eq!(addresses, vec![client_addr.clone()]);
                    assert_eq!(listener_id, old_listener);
                    old_listener_closed = true;
                }
                SwarmEvent::Behaviour(ClientEvent::Relay(
                    relay::client::Event::ReservationReqAccepted {
                        relay_peer_id: peer_id,
                        ..
                    },
                )) => {
                    assert_eq!(relay_peer_id, peer_id);
                    new_reservation_accepted = true;
                }
                SwarmEvent::NewListenAddr {
                    address,
                    listener_id,
                } => {
                    assert_eq!(address, client_addr);
                    assert_eq!(listener_id, new_listener);
                    new_listen_address_reported = true;
                }
                SwarmEvent::ExternalAddrConfirmed { address } => {
                    assert_eq!(address, client_addr);
                    new_external_addr_confirmed = true;
                }
                SwarmEvent::ExternalAddrExpired { address } => {
                    assert_eq!(address, client_addr);
                    old_external_addr_expired = true;
                }
                SwarmEvent::Behaviour(ClientEvent::Ping(_)) => {}
                e => panic!("{e:?}"),
            };

            if old_listener_closed
                && old_external_addr_expired
                && new_reservation_accepted
                && new_listen_address_reported
                && new_external_addr_confirmed
            {
                break;
            }
        }
    });
}

#[test]
fn connect() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let mut relay = build_relay();
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone());
    spawn_swarm_on_pool(&pool, relay);

    let mut dst = build_client();
    let dst_peer_id = *dst.local_peer_id();
    let dst_addr = relay_addr
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit);

    dst.listen_on(dst_addr.clone()).unwrap();

    assert!(pool.run_until(wait_for_dial(&mut dst, relay_peer_id)));

    pool.run_until(wait_for_reservation(
        &mut dst,
        dst_addr.clone(),
        relay_peer_id,
        false, // No renewal.
    ));

    let mut src = build_client();
    let src_peer_id = *src.local_peer_id();

    src.dial(dst_addr.with(Protocol::P2p(dst_peer_id))).unwrap();

    pool.run_until(futures::future::join(
        connection_established_to(&mut src, relay_peer_id, dst_peer_id),
        connection_established_to(&mut dst, relay_peer_id, src_peer_id),
    ));
}

async fn connection_established_to(
    swarm: &mut Swarm<Client>,
    relay_peer_id: PeerId,
    other: PeerId,
) {
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::Dialing {
                peer_id: Some(peer_id),
                ..
            } if peer_id == relay_peer_id => {}
            SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == relay_peer_id => {}
            SwarmEvent::Behaviour(ClientEvent::Ping(ping::Event { peer, .. })) if peer == other => {
                break
            }
            SwarmEvent::Behaviour(ClientEvent::Relay(
                relay::client::Event::OutboundCircuitEstablished { .. },
            )) => {}
            SwarmEvent::Behaviour(ClientEvent::Relay(
                relay::client::Event::InboundCircuitEstablished { src_peer_id, .. },
            )) => {
                assert_eq!(src_peer_id, other);
            }
            SwarmEvent::Behaviour(ClientEvent::Ping(ping::Event { peer, .. }))
                if peer == relay_peer_id => {}
            SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == other => break,
            SwarmEvent::IncomingConnection { send_back_addr, .. } => {
                let peer_id_from_addr = send_back_addr
                    .iter()
                    .find_map(|protocol| match protocol {
                        Protocol::P2p(peer_id) => Some(peer_id),
                        _ => None,
                    })
                    .expect("to have /p2p");

                assert_eq!(peer_id_from_addr, other)
            }
            e => panic!("{e:?}"),
        }
    }
}

#[test]
fn handle_dial_failure() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let relay_peer_id = PeerId::random();

    let mut client = build_client();
    let client_peer_id = *client.local_peer_id();
    let client_addr = relay_addr
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(client_peer_id));

    client.listen_on(client_addr).unwrap();
    assert!(!pool.run_until(wait_for_dial(&mut client, relay_peer_id)));
}

#[test]
fn propagate_reservation_error_to_listener() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let mut relay = build_relay_with_config(relay::Config {
        max_reservations: 0, // Will make us fail to make the reservation
        ..relay::Config::default()
    });
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone());
    spawn_swarm_on_pool(&pool, relay);

    let client_addr = relay_addr
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit);
    let mut client = build_client();

    let reservation_listener = client.listen_on(client_addr.clone()).unwrap();

    // Wait for connection to relay.
    assert!(pool.run_until(wait_for_dial(&mut client, relay_peer_id)));

    let error = pool.run_until(client.wait(|e| match e {
        SwarmEvent::ListenerClosed {
            listener_id,
            reason: Err(e),
            ..
        } if listener_id == reservation_listener => Some(e),
        _ => None,
    }));

    let error = error
        .source()
        .unwrap()
        .downcast_ref::<relay::outbound::hop::ReserveError>()
        .unwrap();

    assert!(matches!(
        error,
        relay::outbound::hop::ReserveError::ResourceLimitExceeded
    ));
}

#[test]
fn propagate_connect_error_to_unknown_peer_to_dialer() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let mut relay = build_relay();
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone());
    spawn_swarm_on_pool(&pool, relay);

    let mut src = build_client();

    let dst_peer_id = PeerId::random(); // We don't have a destination peer in this test, so the CONNECT request will fail.
    let dst_addr = relay_addr
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(dst_peer_id));

    let opts = DialOpts::from(dst_addr.clone());
    let circuit_connection_id = opts.connection_id();

    src.dial(opts).unwrap();

    let (failed_address, error) = pool.run_until(src.wait(|e| match e {
        SwarmEvent::OutgoingConnectionError {
            connection_id,
            error: DialError::Transport(mut errors),
            ..
        } if connection_id == circuit_connection_id => {
            assert_eq!(errors.len(), 1);
            Some(errors.remove(0))
        }
        _ => None,
    }));

    // This is a bit wonky but we need to get the _actual_ source error :)
    let error = error
        .source()
        .unwrap()
        .source()
        .unwrap()
        .downcast_ref::<relay::outbound::hop::ConnectError>()
        .unwrap();

    assert_eq!(failed_address, dst_addr);
    assert!(matches!(
        error,
        relay::outbound::hop::ConnectError::NoReservation
    ));
}

#[test]
fn reuse_connection() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let mut relay = build_relay();
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone());
    spawn_swarm_on_pool(&pool, relay);

    let client_addr = relay_addr
        .clone()
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit);

    // To reuse the connection, we need to ensure it is not shut down due to being idle.
    let mut client = build_client_with_config(
        Config::with_async_std_executor().with_idle_connection_timeout(Duration::from_secs(1)),
    );

    client.dial(relay_addr).unwrap();
    assert!(pool.run_until(wait_for_dial(&mut client, relay_peer_id)));

    client.listen_on(client_addr.clone()).unwrap();

    pool.run_until(wait_for_reservation(
        &mut client,
        client_addr,
        relay_peer_id,
        false, // No renewal.
    ));
}

fn build_relay() -> Swarm<Relay> {
    build_relay_with_config(relay::Config {
        reservation_duration: Duration::from_secs(2),
        ..Default::default()
    })
}

fn build_relay_with_config(config: relay::Config) -> Swarm<Relay> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = local_key.public().to_peer_id();

    let transport = upgrade_transport(MemoryTransport::default().boxed(), &local_key);

    Swarm::new(
        transport,
        Relay {
            ping: ping::Behaviour::new(ping::Config::new()),
            relay: relay::Behaviour::new(local_peer_id, config),
        },
        local_peer_id,
        Config::with_async_std_executor(),
    )
}

fn build_client() -> Swarm<Client> {
    build_client_with_config(Config::with_async_std_executor())
}

fn build_client_with_config(config: Config) -> Swarm<Client> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = local_key.public().to_peer_id();

    let (relay_transport, behaviour) = relay::client::new(local_peer_id);
    let transport = upgrade_transport(
        OrTransport::new(relay_transport, MemoryTransport::default()).boxed(),
        &local_key,
    );

    Swarm::new(
        transport,
        Client {
            ping: ping::Behaviour::new(ping::Config::new()),
            relay: behaviour,
        },
        local_peer_id,
        config,
    )
}

fn upgrade_transport<StreamSink>(
    transport: Boxed<StreamSink>,
    identity: &identity::Keypair,
) -> Boxed<(PeerId, StreamMuxerBox)>
where
    StreamSink: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    transport
        .upgrade(upgrade::Version::V1)
        .authenticate(plaintext::Config::new(identity))
        .multiplex(libp2p_yamux::Config::default())
        .boxed()
}

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct Relay {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
}

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct Client {
    relay: relay::client::Behaviour,
    ping: ping::Behaviour,
}

fn spawn_swarm_on_pool<B: NetworkBehaviour + Send>(pool: &LocalPool, swarm: Swarm<B>) {
    pool.spawner()
        .spawn_obj(swarm.collect::<Vec<_>>().map(|_| ()).boxed().into())
        .unwrap();
}

async fn wait_for_reservation(
    client: &mut Swarm<Client>,
    client_addr: Multiaddr,
    relay_peer_id: PeerId,
    is_renewal: bool,
) {
    let mut new_listen_addr = false;
    let mut reservation_req_accepted = false;
    let mut external_addr_confirmed = false;

    loop {
        match client.select_next_some().await {
            SwarmEvent::ExternalAddrConfirmed { address } if !is_renewal => {
                assert_eq!(address, client_addr);
                external_addr_confirmed = true;
            }
            SwarmEvent::Behaviour(ClientEvent::Relay(
                relay::client::Event::ReservationReqAccepted {
                    relay_peer_id: peer_id,
                    renewal,
                    ..
                },
            )) if relay_peer_id == peer_id && renewal == is_renewal => {
                reservation_req_accepted = true;
            }
            SwarmEvent::NewListenAddr { address, .. } if !is_renewal => {
                assert_eq!(address, client_addr);
                new_listen_addr = true;
            }
            SwarmEvent::Behaviour(ClientEvent::Ping(_)) => {}
            e => panic!("{e:?}"),
        }

        if reservation_req_accepted
            && (external_addr_confirmed || is_renewal)
            && (new_listen_addr || is_renewal)
        {
            break;
        }
    }
}

async fn wait_for_dial(client: &mut Swarm<Client>, remote: PeerId) -> bool {
    loop {
        match client.select_next_some().await {
            SwarmEvent::Dialing {
                peer_id: Some(peer_id),
                ..
            } if peer_id == remote => {}
            SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == remote => return true,
            SwarmEvent::OutgoingConnectionError { peer_id, .. } if peer_id == Some(remote) => {
                return false
            }
            SwarmEvent::Behaviour(ClientEvent::Ping(_)) => {}
            e => panic!("{e:?}"),
        }
    }
}
