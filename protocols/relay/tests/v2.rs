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
use libp2p::core::multiaddr::{Multiaddr, Protocol};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::choice::OrTransport;
use libp2p::core::transport::{Boxed, MemoryTransport, Transport};
use libp2p::core::PublicKey;
use libp2p::core::{identity, upgrade, PeerId};
use libp2p::ping::{Ping, PingConfig, PingEvent};
use libp2p::plaintext::PlainText2Config;
use libp2p::relay::v2::client;
use libp2p::relay::v2::relay;
use libp2p::NetworkBehaviour;
use libp2p_swarm::{AddressScore, NetworkBehaviour, Swarm, SwarmEvent};
use std::time::Duration;

#[test]
fn reservation() {
    let _ = env_logger::try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let mut relay = build_relay();
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone(), AddressScore::Infinite);
    spawn_swarm_on_pool(&pool, relay);

    let client_addr = relay_addr
        .clone()
        .with(Protocol::P2p(relay_peer_id.into()))
        .with(Protocol::P2pCircuit);
    let mut client = build_client();
    let client_peer_id = *client.local_peer_id();

    client.listen_on(client_addr.clone()).unwrap();

    // Wait for connection to relay.
    assert!(pool.run_until(wait_for_dial(&mut client, relay_peer_id)));

    // Wait for initial reservation.
    pool.run_until(wait_for_reservation(
        &mut client,
        client_addr
            .clone()
            .with(Protocol::P2p(client_peer_id.into())),
        relay_peer_id,
        false, // No renewal.
    ));

    // Wait for renewal.
    pool.run_until(wait_for_reservation(
        &mut client,
        client_addr.with(Protocol::P2p(client_peer_id.into())),
        relay_peer_id,
        true, // Renewal.
    ));
}

#[test]
fn new_reservation_to_same_relay_replaces_old() {
    let _ = env_logger::try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let mut relay = build_relay();
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone(), AddressScore::Infinite);
    spawn_swarm_on_pool(&pool, relay);

    let mut client = build_client();
    let client_peer_id = *client.local_peer_id();
    let client_addr = relay_addr
        .clone()
        .with(Protocol::P2p(relay_peer_id.into()))
        .with(Protocol::P2pCircuit);
    let client_addr_with_peer_id = client_addr
        .clone()
        .with(Protocol::P2p(client_peer_id.into()));

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
    let new_listener = client.listen_on(client_addr.clone()).unwrap();

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
                    client::Event::ReservationReqAccepted {
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
                e => panic!("{:?}", e),
            }
        }
    });
}

#[test]
fn connect() {
    let _ = env_logger::try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let mut relay = build_relay();
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone(), AddressScore::Infinite);
    spawn_swarm_on_pool(&pool, relay);

    let mut dst = build_client();
    let dst_peer_id = *dst.local_peer_id();
    let dst_addr = relay_addr
        .clone()
        .with(Protocol::P2p(relay_peer_id.into()))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(dst_peer_id.into()));

    dst.listen_on(dst_addr.clone()).unwrap();

    assert!(pool.run_until(wait_for_dial(&mut dst, relay_peer_id)));

    pool.run_until(wait_for_reservation(
        &mut dst,
        dst_addr.clone(),
        relay_peer_id,
        false, // No renewal.
    ));
    spawn_swarm_on_pool(&pool, dst);

    let mut src = build_client();

    src.dial(dst_addr).unwrap();

    pool.run_until(async {
        loop {
            match src.select_next_some().await {
                SwarmEvent::Dialing(peer_id) if peer_id == relay_peer_id => {}
                SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == relay_peer_id => {}
                SwarmEvent::Behaviour(ClientEvent::Ping(PingEvent { peer, .. }))
                    if peer == dst_peer_id =>
                {
                    break
                }
                SwarmEvent::Behaviour(ClientEvent::Relay(
                    client::Event::OutboundCircuitEstablished { .. },
                )) => {}
                SwarmEvent::Behaviour(ClientEvent::Ping(PingEvent { peer, .. }))
                    if peer == relay_peer_id => {}
                SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == dst_peer_id => {
                    break
                }
                e => panic!("{:?}", e),
            }
        }
    })
}

#[test]
fn handle_dial_failure() {
    let _ = env_logger::try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let relay_peer_id = PeerId::random();

    let mut client = build_client();
    let client_peer_id = *client.local_peer_id();
    let client_addr = relay_addr
        .clone()
        .with(Protocol::P2p(relay_peer_id.into()))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(client_peer_id.into()));

    client.listen_on(client_addr.clone()).unwrap();
    assert!(!pool.run_until(wait_for_dial(&mut client, relay_peer_id)));
}

#[test]
fn reuse_connection() {
    let _ = env_logger::try_init();
    let mut pool = LocalPool::new();

    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let mut relay = build_relay();
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone(), AddressScore::Infinite);
    spawn_swarm_on_pool(&pool, relay);

    let client_addr = relay_addr
        .clone()
        .with(Protocol::P2p(relay_peer_id.into()))
        .with(Protocol::P2pCircuit);
    let mut client = build_client();
    let client_peer_id = *client.local_peer_id();

    client.dial(relay_addr).unwrap();
    assert!(pool.run_until(wait_for_dial(&mut client, relay_peer_id)));

    client.listen_on(client_addr.clone()).unwrap();

    pool.run_until(wait_for_reservation(
        &mut client,
        client_addr.with(Protocol::P2p(client_peer_id.into())),
        relay_peer_id,
        false, // No renewal.
    ));
}

fn build_relay() -> Swarm<Relay> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_public_key = local_key.public();
    let local_peer_id = local_public_key.clone().to_peer_id();

    let transport = upgrade_transport(MemoryTransport::default().boxed(), local_public_key);

    Swarm::new(
        transport,
        Relay {
            ping: Ping::new(PingConfig::new()),
            relay: relay::Relay::new(
                local_peer_id,
                relay::Config {
                    reservation_duration: Duration::from_secs(2),
                    ..Default::default()
                },
            ),
        },
        local_peer_id,
    )
}

fn build_client() -> Swarm<Client> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_public_key = local_key.public();
    let local_peer_id = local_public_key.clone().to_peer_id();

    let (relay_transport, behaviour) = client::Client::new_transport_and_behaviour(local_peer_id);
    let transport = upgrade_transport(
        OrTransport::new(relay_transport, MemoryTransport::default()).boxed(),
        local_public_key,
    );

    Swarm::new(
        transport,
        Client {
            ping: Ping::new(PingConfig::new()),
            relay: behaviour,
        },
        local_peer_id,
    )
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
        .multiplex(libp2p_yamux::YamuxConfig::default())
        .boxed()
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "RelayEvent", event_process = false)]
struct Relay {
    relay: relay::Relay,
    ping: Ping,
}

#[derive(Debug)]
enum RelayEvent {
    Relay(relay::Event),
    Ping(PingEvent),
}

impl From<relay::Event> for RelayEvent {
    fn from(event: relay::Event) -> Self {
        RelayEvent::Relay(event)
    }
}

impl From<PingEvent> for RelayEvent {
    fn from(event: PingEvent) -> Self {
        RelayEvent::Ping(event)
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "ClientEvent", event_process = false)]
struct Client {
    relay: client::Client,
    ping: Ping,
}

#[derive(Debug)]
enum ClientEvent {
    Relay(client::Event),
    Ping(PingEvent),
}

impl From<client::Event> for ClientEvent {
    fn from(event: client::Event) -> Self {
        ClientEvent::Relay(event)
    }
}

impl From<PingEvent> for ClientEvent {
    fn from(event: PingEvent) -> Self {
        ClientEvent::Ping(event)
    }
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
            SwarmEvent::Behaviour(ClientEvent::Relay(client::Event::ReservationReqAccepted {
                relay_peer_id: peer_id,
                renewal,
                ..
            })) if relay_peer_id == peer_id && renewal == is_renewal => {
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
            e => panic!("{:?}", e),
        }
    }
}

async fn wait_for_dial(client: &mut Swarm<Client>, remote: PeerId) -> bool {
    loop {
        match client.select_next_some().await {
            SwarmEvent::Dialing(peer_id) if peer_id == remote => {}
            SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == remote => return true,
            SwarmEvent::OutgoingConnectionError { peer_id, .. } if peer_id == Some(remote) => {
                return false
            }
            SwarmEvent::Behaviour(ClientEvent::Ping(_)) => {}
            e => panic!("{:?}", e),
        }
    }
}
