use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use futures::{
    io::{AsyncRead, AsyncWrite},
    stream::StreamExt,
};
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    muxing::StreamMuxerBox,
    transport::{Boxed, MemoryTransport, Transport, choice::OrTransport},
    upgrade,
};
use libp2p_identify as identify;
use libp2p_identity as identity;
use libp2p_identity::PeerId;
use libp2p_plaintext as plaintext;
use libp2p_relay::{self as relay, autorelay};
use libp2p_swarm::{Config, ConnectionId, NetworkBehaviour, Swarm, SwarmEvent};
use tracing_subscriber::EnvFilter;

#[tokio::test]
async fn autorelay_reserves_when_peer_supports_hop() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut relay = build_relay();
    let relay_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let relay_peer_id = *relay.local_peer_id();

    relay.listen_on(relay_addr.clone()).unwrap();
    relay.add_external_address(relay_addr.clone());
    tokio::spawn(async move {
        relay.collect::<Vec<_>>().await;
    });

    let mut client = build_client(autorelay::Config::default());
    let client_peer_id = *client.local_peer_id();
    client.dial(relay_addr.clone()).unwrap();

    let expected_relayed = relay_addr
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(client_peer_id));

    wait_until(&mut client, Duration::from_secs(20), {
        let mut new_listen_addr = false;
        let mut reservation_accepted = false;
        move |event| {
            match event {
                SwarmEvent::NewListenAddr { address, .. } if address == &expected_relayed => {
                    new_listen_addr = true;
                }
                SwarmEvent::Behaviour(ClientEvent::RelayClient(
                    relay::client::Event::ReservationReqAccepted { .. },
                )) => {
                    reservation_accepted = true;
                }
                _ => {}
            }
            new_listen_addr && reservation_accepted
        }
    })
    .await;
}

#[tokio::test]
async fn autorelay_respects_max_reservations() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut relay_a = build_relay();
    let relay_a_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let relay_a_peer_id = *relay_a.local_peer_id();
    relay_a.listen_on(relay_a_addr.clone()).unwrap();
    relay_a.add_external_address(relay_a_addr.clone());
    tokio::spawn(async move {
        relay_a.collect::<Vec<_>>().await;
    });

    let mut relay_b = build_relay();
    let relay_b_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let relay_b_peer_id = *relay_b.local_peer_id();
    relay_b.listen_on(relay_b_addr.clone()).unwrap();
    relay_b.add_external_address(relay_b_addr.clone());
    tokio::spawn(async move {
        relay_b.collect::<Vec<_>>().await;
    });

    let mut client = build_client(autorelay::Config::default().set_max_reservations(1));
    client.dial(relay_a_addr.clone()).unwrap();
    client.dial(relay_b_addr.clone()).unwrap();

    let mut accepted = 0usize;
    let timeout = tokio::time::sleep(Duration::from_secs(20));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            _ = &mut timeout => break,
            ev = client.select_next_some() => {
                if let SwarmEvent::Behaviour(ClientEvent::RelayClient(
                    relay::client::Event::ReservationReqAccepted { relay_peer_id, .. },
                )) = ev
                {
                    assert!(relay_peer_id == relay_a_peer_id || relay_peer_id == relay_b_peer_id);
                    accepted += 1;
                    if accepted > 1 {
                        panic!("autorelay opened more reservations than max_reservations=1");
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    break;
                }
            }
        }
    }

    assert_eq!(
        accepted, 1,
        "expected exactly one reservation, observed {accepted}"
    );
}

#[tokio::test]
async fn autorelay_refills_after_connection_drop() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut relay_a = build_relay();
    let relay_a_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let relay_a_peer_id = *relay_a.local_peer_id();
    relay_a.listen_on(relay_a_addr.clone()).unwrap();
    relay_a.add_external_address(relay_a_addr.clone());
    tokio::spawn(async move {
        relay_a.collect::<Vec<_>>().await;
    });

    let mut relay_b = build_relay();
    let relay_b_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    let relay_b_peer_id = *relay_b.local_peer_id();
    relay_b.listen_on(relay_b_addr.clone()).unwrap();
    relay_b.add_external_address(relay_b_addr.clone());
    tokio::spawn(async move {
        relay_b.collect::<Vec<_>>().await;
    });

    let mut client = build_client(autorelay::Config::default().set_max_reservations(1));
    client.dial(relay_a_addr.clone()).unwrap();
    client.dial(relay_b_addr.clone()).unwrap();

    let (first_relay, first_conn) = wait_for_reservation_from_either(
        &mut client,
        relay_a_peer_id,
        relay_b_peer_id,
        Duration::from_secs(20),
    )
    .await;

    assert!(
        client.close_connection(first_conn),
        "first reservation connection should exist"
    );

    let other_relay = if first_relay == relay_a_peer_id {
        relay_b_peer_id
    } else {
        relay_a_peer_id
    };

    wait_until(&mut client, Duration::from_secs(20), {
        let mut got = false;
        move |event| {
            if let SwarmEvent::Behaviour(ClientEvent::RelayClient(
                relay::client::Event::ReservationReqAccepted { relay_peer_id, .. },
            )) = event
                && *relay_peer_id == other_relay
            {
                got = true;
            }
            got
        }
    })
    .await;
}

#[tokio::test]
async fn autorelay_with_two_reservations_among_five_relays() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut relay_addrs: Vec<(PeerId, Multiaddr)> = Vec::with_capacity(5);
    for _ in 0..5 {
        let mut relay = build_relay();
        let addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
        let peer_id = *relay.local_peer_id();
        relay.listen_on(addr.clone()).unwrap();
        relay.add_external_address(addr.clone());
        relay_addrs.push((peer_id, addr.clone()));
        tokio::spawn(async move {
            relay.collect::<Vec<_>>().await;
        });
    }

    let relay_peers: HashSet<PeerId> = relay_addrs.iter().map(|(p, _)| *p).collect();

    let mut client = build_client(autorelay::Config::default().set_max_reservations(2));
    for (_, addr) in &relay_addrs {
        client.dial(addr.clone()).unwrap();
    }

    let mut direct_conns: HashMap<PeerId, ConnectionId> = HashMap::new();
    let mut reservations: HashSet<PeerId> = HashSet::new();

    let sleep = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => panic!(
                "timeout: got {} reservations, expected 2",
                reservations.len()
            ),
            ev = client.select_next_some() => match ev {
                SwarmEvent::ConnectionEstablished {
                    peer_id, connection_id, endpoint, ..
                } if !endpoint.is_relayed() && relay_peers.contains(&peer_id) => {
                    direct_conns.insert(peer_id, connection_id);
                }
                SwarmEvent::Behaviour(ClientEvent::RelayClient(
                    relay::client::Event::ReservationReqAccepted {
                        relay_peer_id,
                        renewal: false,
                        ..
                    },
                )) => {
                    reservations.insert(relay_peer_id);
                }
                _ => {}
            }
        }
        if reservations.len() == 2 {
            break;
        }
    }

    let drop_peer = *reservations.iter().next().expect("two reservations held");
    let keep_peer = reservations
        .iter()
        .find(|p| **p != drop_peer)
        .copied()
        .expect("two reservations held");
    let drop_conn = *direct_conns
        .get(&drop_peer)
        .expect("direct connection observed");

    assert!(
        client.close_connection(drop_conn),
        "should close the relay connection holding a reservation"
    );

    let sleep = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => panic!("timeout waiting for replacement reservation"),
            ev = client.select_next_some() => {
                if let SwarmEvent::Behaviour(ClientEvent::RelayClient(
                    relay::client::Event::ReservationReqAccepted {
                        relay_peer_id,
                        renewal: false,
                        ..
                    },
                )) = ev
                    && relay_peer_id != keep_peer
                    && relay_peer_id != drop_peer
                {
                    return;
                }
            }
        }
    }
}

#[tokio::test]
async fn autorelay_drops_reservations_when_public_address_appears() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let mut relay_a = build_relay();
    let relay_a_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    relay_a.listen_on(relay_a_addr.clone()).unwrap();
    relay_a.add_external_address(relay_a_addr.clone());
    tokio::spawn(async move {
        relay_a.collect::<Vec<_>>().await;
    });

    let mut relay_b = build_relay();
    let relay_b_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    relay_b.listen_on(relay_b_addr.clone()).unwrap();
    relay_b.add_external_address(relay_b_addr.clone());
    tokio::spawn(async move {
        relay_b.collect::<Vec<_>>().await;
    });

    let mut client = build_client(autorelay::Config::default().set_max_reservations(2));
    client.dial(relay_a_addr.clone()).unwrap();
    client.dial(relay_b_addr.clone()).unwrap();

    let mut confirmed: HashSet<Multiaddr> = HashSet::new();
    let sleep = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => panic!(
                "timeout: got {} confirmed external addresses, expected 2",
                confirmed.len()
            ),
            ev = client.select_next_some() => {
                if let SwarmEvent::ExternalAddrConfirmed { address } = ev
                    && address.iter().any(|p| p == Protocol::P2pCircuit)
                {
                    confirmed.insert(address);
                }
            }
        }
        if confirmed.len() == 2 {
            break;
        }
    }

    let public_addr = Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()));
    client.add_external_address(public_addr);

    let mut expired: HashSet<Multiaddr> = HashSet::new();
    let sleep = tokio::time::sleep(Duration::from_secs(15));
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => panic!(
                "timeout: only {}/{} relayed addresses expired",
                expired.len(),
                confirmed.len()
            ),
            ev = client.select_next_some() => {
                if let SwarmEvent::ExternalAddrExpired { address } = ev
                    && confirmed.contains(&address)
                {
                    expired.insert(address);
                }
            }
        }
        if expired == confirmed {
            break;
        }
    }
}

async fn wait_for_reservation_from_either(
    client: &mut Swarm<Client>,
    peer_a: PeerId,
    peer_b: PeerId,
    timeout: Duration,
) -> (PeerId, ConnectionId) {
    let sleep = tokio::time::sleep(timeout);
    tokio::pin!(sleep);
    let mut direct_conns: HashMap<PeerId, ConnectionId> = HashMap::new();
    loop {
        tokio::select! {
            _ = &mut sleep => panic!("timeout waiting for reservation from either relay"),
            ev = client.select_next_some() => {
                match ev {
                    SwarmEvent::ConnectionEstablished {
                        peer_id,
                        connection_id,
                        endpoint,
                        ..
                    } if (peer_id == peer_a || peer_id == peer_b)
                        && !endpoint.is_relayed() =>
                    {
                        direct_conns.insert(peer_id, connection_id);
                    }
                    SwarmEvent::Behaviour(ClientEvent::RelayClient(
                        relay::client::Event::ReservationReqAccepted { relay_peer_id, .. },
                    )) if relay_peer_id == peer_a || relay_peer_id == peer_b => {
                        let conn_id = direct_conns
                            .get(&relay_peer_id)
                            .copied()
                            .expect("direct connection to relay was observed");
                        return (relay_peer_id, conn_id);
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn wait_until<F>(client: &mut Swarm<Client>, timeout: Duration, mut predicate: F)
where
    F: FnMut(&SwarmEvent<ClientEvent>) -> bool,
{
    let sleep = tokio::time::sleep(timeout);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => panic!("timeout waiting on predicate"),
            ev = client.select_next_some() => {
                if predicate(&ev) {
                    return;
                }
            }
        }
    }
}

fn build_relay() -> Swarm<Relay> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = local_key.public().to_peer_id();
    let transport = upgrade_transport(MemoryTransport::default().boxed(), &local_key);

    Swarm::new(
        transport,
        Relay {
            relay: relay::Behaviour::new(
                local_peer_id,
                relay::Config {
                    reservation_duration: Duration::from_secs(60),
                    ..Default::default()
                },
            ),
            identify: identify::Behaviour::new(identify::Config::new(
                "/autorelay-test/1.0.0".to_owned(),
                local_key.public(),
            )),
        },
        local_peer_id,
        Config::with_tokio_executor(),
    )
}

fn build_client(autorelay_config: autorelay::Config) -> Swarm<Client> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = local_key.public().to_peer_id();
    let (relay_transport, relay_client) = relay::client::new(local_peer_id);

    let transport = upgrade_transport(
        OrTransport::new(relay_transport, MemoryTransport::default()).boxed(),
        &local_key,
    );

    Swarm::new(
        transport,
        Client {
            relay_client,
            autorelay: autorelay::Behaviour::new_with_config(autorelay_config),
            identify: identify::Behaviour::new(identify::Config::new(
                "/autorelay-test/1.0.0".to_owned(),
                local_key.public(),
            )),
        },
        local_peer_id,
        Config::with_tokio_executor(),
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
    identify: identify::Behaviour,
}

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct Client {
    relay_client: relay::client::Behaviour,
    autorelay: autorelay::Behaviour,
    identify: identify::Behaviour,
}
