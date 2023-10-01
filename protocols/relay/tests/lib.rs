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
use libp2p_identity::PublicKey;
use libp2p_ping as ping;
use libp2p_plaintext::PlainText2Config;
use libp2p_relay as relay;
use libp2p_swarm::{NetworkBehaviour, Swarm, SwarmBuilder, SwarmEvent};
use std::time::Duration;
use tracing_subscriber::EnvFilter;

#[test]
fn reservation() {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
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
    let client_peer_id = *client.local_peer_id();

    client.listen_on(client_addr.clone()).unwrap();

    // Wait for connection to relay.
    assert!(pool.run_until(wait_for_dial(&mut client, relay_peer_id)));

    // Wait for initial reservation.
    pool.run_until(wait_for_reservation(
        &mut client,
        client_addr.clone().with(Protocol::P2p(client_peer_id)),
        relay_peer_id,
        false, // No renewal.
    ));

    // Wait for renewal.
    pool.run_until(wait_for_reservation(
        &mut client,
        client_addr.with(Protocol::P2p(client_peer_id)),
        relay_peer_id,
        true, // Renewal.
    ));
}

#[test]
fn new_reservation_to_same_relay_replaces_old() {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let mut relay = build_relay();
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone());
    spawn_swarm_on_pool(&pool, relay);

    let mut client = build_client();
    let client_peer_id = *client.local_peer_id();
    let client_addr = relay_addr
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit);
    let client_addr_with_peer_id = client_addr.clone().with(Protocol::P2p(client_peer_id));

    let old_listener = client.listen_on(client_addr.clone()).unwrap();

    // Wait for connection to relay.
    assert!(pool.run_until(wait_for_dial(&mut client, relay_peer_id)));

    // Wait for first (old) reservation.
    pool.run_until(wait_for_reservation(
        &mut client,
        client_addr_with_peer_id.clone(),
        relay_peer_id,
        false, // No renewal.
    ));

    // Trigger new reservation.
    let new_listener = client.listen_on(client_addr).unwrap();

    // Wait for
    // - listener of old reservation to close
    // - new reservation to be accepted
    // - new listener address to be reported
    pool.run_until(async {
        let mut old_listener_closed = false;
        let mut new_reservation_accepted = false;
        let mut new_listener_address_reported = false;
        loop {
            match client.select_next_some().await {
                SwarmEvent::ListenerClosed {
                    addresses,
                    listener_id,
                    ..
                } => {
                    assert_eq!(addresses, vec![client_addr_with_peer_id.clone()]);
                    assert_eq!(listener_id, old_listener);

                    old_listener_closed = true;
                    if new_reservation_accepted && new_listener_address_reported {
                        break;
                    }
                }
                SwarmEvent::Behaviour(ClientEvent::Relay(
                    relay::client::Event::ReservationReqAccepted {
                        relay_peer_id: peer_id,
                        ..
                    },
                )) => {
                    assert_eq!(relay_peer_id, peer_id);

                    new_reservation_accepted = true;
                    if old_listener_closed && new_listener_address_reported {
                        break;
                    }
                }
                SwarmEvent::NewListenAddr {
                    address,
                    listener_id,
                } => {
                    assert_eq!(address, client_addr_with_peer_id);
                    assert_eq!(listener_id, new_listener);

                    new_listener_address_reported = true;
                    if old_listener_closed && new_reservation_accepted {
                        break;
                    }
                }
                SwarmEvent::Behaviour(ClientEvent::Ping(_)) => {}
                e => panic!("{e:?}"),
            }
        }
    });
}

#[test]
fn connect() {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
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
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(dst_peer_id));

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

    src.dial(dst_addr).unwrap();

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
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
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
fn reuse_connection() {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
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
    let mut client = build_client();
    let client_peer_id = *client.local_peer_id();

    client.dial(relay_addr).unwrap();
    assert!(pool.run_until(wait_for_dial(&mut client, relay_peer_id)));

    client.listen_on(client_addr.clone()).unwrap();

    pool.run_until(wait_for_reservation(
        &mut client,
        client_addr.with(Protocol::P2p(client_peer_id)),
        relay_peer_id,
        false, // No renewal.
    ));
}

fn build_relay() -> Swarm<Relay> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_public_key = local_key.public();
    let local_peer_id = local_public_key.to_peer_id();

    let transport = upgrade_transport(MemoryTransport::default().boxed(), local_public_key);

    SwarmBuilder::with_async_std_executor(
        transport,
        Relay {
            ping: ping::Behaviour::new(ping::Config::new()),
            relay: relay::Behaviour::new(
                local_peer_id,
                relay::Config {
                    reservation_duration: Duration::from_secs(2),
                    ..Default::default()
                },
            ),
        },
        local_peer_id,
    )
    .build()
}

fn build_client() -> Swarm<Client> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_public_key = local_key.public();
    let local_peer_id = local_public_key.to_peer_id();

    let (relay_transport, behaviour) = relay::client::new(local_peer_id);
    let transport = upgrade_transport(
        OrTransport::new(relay_transport, MemoryTransport::default()).boxed(),
        local_public_key,
    );

    SwarmBuilder::with_async_std_executor(
        transport,
        Client {
            ping: ping::Behaviour::new(ping::Config::new()),
            relay: behaviour,
        },
        local_peer_id,
    )
    .build()
}

fn upgrade_transport<StreamSink>(
    transport: Boxed<StreamSink>,
    local_public_key: PublicKey,
) -> Boxed<(PeerId, StreamMuxerBox)>
where
    StreamSink: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    transport
        .upgrade(upgrade::Version::V1)
        .authenticate(PlainText2Config { local_public_key })
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

    loop {
        match client.select_next_some().await {
            SwarmEvent::Behaviour(ClientEvent::Relay(
                relay::client::Event::ReservationReqAccepted {
                    relay_peer_id: peer_id,
                    renewal,
                    ..
                },
            )) if relay_peer_id == peer_id && renewal == is_renewal => {
                reservation_req_accepted = true;
                if new_listen_addr {
                    break;
                }
            }
            SwarmEvent::NewListenAddr { address, .. } if address == client_addr => {
                new_listen_addr = true;
                if reservation_req_accepted {
                    break;
                }
            }
            SwarmEvent::Behaviour(ClientEvent::Ping(_)) => {}
            e => panic!("{e:?}"),
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
