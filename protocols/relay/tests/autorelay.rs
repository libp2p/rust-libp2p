use std::{
    collections::{HashMap, HashSet},
    future::Future,
    num::NonZeroU8,
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
async fn autorelay_respects_max_reservations() {
    init_tracing();

    let (relay_a_peer_id, relay_a_addr) = spawn_relay();
    let (relay_b_peer_id, relay_b_addr) = spawn_relay();

    let mut client =
        build_client(autorelay::Config::default().set_max_reservations(NonZeroU8::new(1).unwrap()));
    client.dial(relay_a_addr).unwrap();
    client.dial(relay_b_addr).unwrap();

    let mut accepted = 0usize;
    let mut timeout = futures_timer::Delay::new(Duration::from_secs(20));
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
                    futures_timer::Delay::new(Duration::from_secs(2)).await;
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
async fn autorelay_with_two_reservations_among_five_relays() {
    init_tracing();

    let relay_addrs: Vec<(PeerId, Multiaddr)> = (0..5).map(|_| spawn_relay()).collect();
    let relay_peers: HashSet<PeerId> = relay_addrs.iter().map(|(p, _)| *p).collect();

    let mut client =
        build_client(autorelay::Config::default().set_max_reservations(NonZeroU8::new(2).unwrap()));
    for (_, addr) in &relay_addrs {
        client.dial(addr.clone()).unwrap();
    }

    let mut direct_conns: HashMap<PeerId, ConnectionId> = HashMap::new();
    let mut reservations: HashSet<PeerId> = HashSet::new();

    let mut sleep = futures_timer::Delay::new(Duration::from_secs(30));
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

    let mut sleep = futures_timer::Delay::new(Duration::from_secs(30));

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
    init_tracing();

    let (_, relay_a_addr) = spawn_relay();
    let (_, relay_b_addr) = spawn_relay();

    let mut client =
        build_client(autorelay::Config::default().set_max_reservations(NonZeroU8::new(2).unwrap()));
    client.dial(relay_a_addr).unwrap();
    client.dial(relay_b_addr).unwrap();

    let mut confirmed: HashSet<Multiaddr> = HashSet::new();
    let mut sleep = futures_timer::Delay::new(Duration::from_secs(30));

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
    let mut sleep = futures_timer::Delay::new(Duration::from_secs(15));

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

#[tokio::test]
async fn autorelay_blacklists_failing_relay_and_retries_after_cooldown() {
    init_tracing();

    let (_, relay_addr) = spawn_rejecting_relay();

    let cooldown = Duration::from_secs(1);
    let mut client = build_client(
        autorelay::Config::default()
            .set_max_reservations(NonZeroU8::new(1).unwrap())
            .set_failure_cooldown(cooldown),
    );
    client.dial(relay_addr).unwrap();

    let first_failure_at = wait_for_listener_failure(&mut client, Duration::from_secs(10)).await;

    let early_retry = with_timeout(
        wait_for_listener_failure(&mut client, cooldown * 5),
        cooldown / 2,
    )
    .await;
    assert!(
        early_retry.is_none(),
        "autorelay retried during the cooldown window"
    );

    let second_failure_at = wait_for_listener_failure(&mut client, cooldown * 5).await;
    let elapsed = second_failure_at.duration_since(first_failure_at);
    assert!(
        elapsed >= cooldown,
        "retry should respect cooldown (elapsed {elapsed:?}, cooldown {cooldown:?})"
    );
}

async fn wait_for_listener_failure(
    client: &mut Swarm<Client>,
    timeout: Duration,
) -> std::time::Instant {
    let mut sleep = futures_timer::Delay::new(timeout);

    loop {
        tokio::select! {
            _ = &mut sleep => panic!("timeout waiting for listener failure"),
            ev = client.select_next_some() => {
                if let SwarmEvent::ListenerClosed { reason: Err(_), .. } = ev {
                    return std::time::Instant::now();
                }
            }
        }
    }
}

#[tokio::test]
async fn autorelay_disabled_does_not_reserve() {
    init_tracing();

    let (_, relay_addr) = spawn_relay();

    let mut client = build_client(autorelay::Config::default());
    client
        .behaviour_mut()
        .autorelay
        .set_status(Some(autorelay::Status::Disable));
    client.dial(relay_addr).unwrap();

    let observed = with_timeout(
        wait_until(&mut client, Duration::from_secs(5), |event| {
            matches!(
                event,
                SwarmEvent::Behaviour(ClientEvent::RelayClient(
                    relay::client::Event::ReservationReqAccepted { .. }
                ))
            )
        }),
        Duration::from_secs(3),
    )
    .await;

    assert!(
        observed.is_none(),
        "autorelay opened a reservation while disabled"
    );
}

#[tokio::test]
async fn autorelay_re_enable_triggers_reservation() {
    init_tracing();

    let (_, relay_addr) = spawn_relay();

    let mut client = build_client(autorelay::Config::default());
    client
        .behaviour_mut()
        .autorelay
        .set_status(Some(autorelay::Status::Disable));
    client.dial(relay_addr).unwrap();

    let mut sleep = futures_timer::Delay::new(Duration::from_secs(3));

    loop {
        tokio::select! {
            _ = &mut sleep => break,
            ev = client.select_next_some() => {
                if matches!(
                    ev,
                    SwarmEvent::Behaviour(ClientEvent::RelayClient(
                        relay::client::Event::ReservationReqAccepted { .. }
                    ))
                ) {
                    panic!("autorelay reserved while disabled");
                }
            }
        }
    }

    client
        .behaviour_mut()
        .autorelay
        .set_status(Some(autorelay::Status::Enable));

    wait_until(&mut client, Duration::from_secs(10), |event| {
        matches!(
            event,
            SwarmEvent::Behaviour(ClientEvent::RelayClient(
                relay::client::Event::ReservationReqAccepted { .. }
            ))
        )
    })
    .await;
}

#[tokio::test]
async fn autorelay_disable_preserves_active_reservation() {
    init_tracing();

    let (_, relay_addr) = spawn_relay();

    let mut client = build_client(autorelay::Config::default());
    client.dial(relay_addr).unwrap();

    wait_until(&mut client, Duration::from_secs(20), |event| {
        matches!(
            event,
            SwarmEvent::Behaviour(ClientEvent::RelayClient(
                relay::client::Event::ReservationReqAccepted { .. }
            ))
        )
    })
    .await;

    client
        .behaviour_mut()
        .autorelay
        .set_status(Some(autorelay::Status::Disable));

    let mut sleep = futures_timer::Delay::new(Duration::from_secs(3));

    loop {
        tokio::select! {
            _ = &mut sleep => break,
            ev = client.select_next_some() => {
                if let SwarmEvent::ListenerClosed { reason: Err(_), .. } = ev {
                    panic!("disabling autorelay dropped an active reservation");
                }
                if let SwarmEvent::ExternalAddrExpired { .. } = ev {
                    panic!("disabling autorelay expired an external address");
                }
            }
        }
    }
}

#[tokio::test]
async fn autorelay_prefers_static_relay() {
    init_tracing();

    let (opportunistic_peer, opportunistic_addr) = spawn_relay();
    let (static_peer, static_addr) = spawn_relay();

    let mut client =
        build_client(autorelay::Config::default().set_max_reservations(NonZeroU8::new(1).unwrap()));
    client
        .behaviour_mut()
        .autorelay
        .set_status(Some(autorelay::Status::Disable));

    client.dial(opportunistic_addr).unwrap();
    client
        .behaviour_mut()
        .autorelay
        .add_static_relay(static_peer, static_addr);

    // Let both connections establish and identify exchanges complete.
    let mut warmup = futures_timer::Delay::new(Duration::from_secs(3));
    loop {
        tokio::select! {
            _ = &mut warmup => break,
            _ = client.select_next_some() => {}
        }
    }

    client
        .behaviour_mut()
        .autorelay
        .set_status(Some(autorelay::Status::Enable));

    let accepted_peer = wait_until_some(&mut client, Duration::from_secs(15), |event| {
        if let SwarmEvent::Behaviour(ClientEvent::RelayClient(
            relay::client::Event::ReservationReqAccepted { relay_peer_id, .. },
        )) = event
        {
            Some(*relay_peer_id)
        } else {
            None
        }
    })
    .await;

    assert_eq!(
        accepted_peer, static_peer,
        "autorelay should pick the static relay over the opportunistic one"
    );
    assert_ne!(accepted_peer, opportunistic_peer);
}

#[tokio::test]
async fn remove_static_relay_preserves_active_reservation() {
    init_tracing();

    let (relay_peer, relay_addr) = spawn_relay();

    let mut client = build_client(autorelay::Config::default());
    client
        .behaviour_mut()
        .autorelay
        .add_static_relay(relay_peer, relay_addr);

    wait_until(&mut client, Duration::from_secs(15), |event| {
        matches!(
            event,
            SwarmEvent::Behaviour(ClientEvent::RelayClient(
                relay::client::Event::ReservationReqAccepted { .. }
            ))
        )
    })
    .await;

    assert!(
        client
            .behaviour_mut()
            .autorelay
            .remove_static_relay(&relay_peer)
    );

    let mut sleep = futures_timer::Delay::new(Duration::from_secs(3));

    loop {
        tokio::select! {
            _ = &mut sleep => break,
            ev = client.select_next_some() => {
                if let SwarmEvent::ListenerClosed { reason: Err(_), .. } = ev {
                    panic!("removing static relay dropped an active reservation");
                }
                if let SwarmEvent::ExternalAddrExpired { .. } = ev {
                    panic!("removing static relay expired an external address");
                }
            }
        }
    }
}

#[tokio::test]
async fn static_relay_redials_after_connection_drop() {
    init_tracing();

    let (relay_peer, relay_addr) = spawn_relay();

    let mut client = build_client(autorelay::Config::default());
    client
        .behaviour_mut()
        .autorelay
        .add_static_relay(relay_peer, relay_addr);

    let conn_id =
        wait_for_reservation_with_conn(&mut client, relay_peer, Duration::from_secs(15)).await;

    assert!(client.close_connection(conn_id));

    wait_until(&mut client, Duration::from_secs(20), {
        let mut redialed = false;
        let mut reserved_again = false;
        move |event| {
            match event {
                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } if *peer_id == relay_peer && !endpoint.is_relayed() => {
                    redialed = true;
                }
                SwarmEvent::Behaviour(ClientEvent::RelayClient(
                    relay::client::Event::ReservationReqAccepted { relay_peer_id, .. },
                )) if *relay_peer_id == relay_peer => {
                    reserved_again = true;
                }
                _ => {}
            }
            redialed && reserved_again
        }
    })
    .await;
}

async fn wait_until_some<F, T>(client: &mut Swarm<Client>, timeout: Duration, mut extract: F) -> T
where
    F: FnMut(&SwarmEvent<ClientEvent>) -> Option<T>,
{
    let mut sleep = futures_timer::Delay::new(timeout);

    loop {
        tokio::select! {
            _ = &mut sleep => panic!("timeout waiting on predicate"),
            ev = client.select_next_some() => {
                if let Some(value) = extract(&ev) {
                    return value;
                }
            }
        }
    }
}

#[tokio::test]
async fn autorelay_emits_relay_available_after_recovery() {
    init_tracing();

    let (relay_peer, relay_addr) = spawn_relay();

    let mut client = build_client(autorelay::Config::default());
    client.dial(relay_addr.clone()).unwrap();

    let conn_id =
        wait_for_reservation_with_conn(&mut client, relay_peer, Duration::from_secs(15)).await;

    assert!(client.close_connection(conn_id));

    wait_until(&mut client, Duration::from_secs(10), |event| {
        matches!(
            event,
            SwarmEvent::Behaviour(ClientEvent::Autorelay(autorelay::Event::NoRelaysAvailable))
        )
    })
    .await;

    client.dial(relay_addr).unwrap();

    wait_until(&mut client, Duration::from_secs(15), |event| {
        matches!(
            event,
            SwarmEvent::Behaviour(ClientEvent::Autorelay(autorelay::Event::RelaysAvailable))
        )
    })
    .await;
}

#[tokio::test]
async fn autorelay_no_relays_available_is_edge_triggered() {
    init_tracing();

    let (relay_a_peer, relay_a_addr) = spawn_relay();
    let (relay_b_peer, relay_b_addr) = spawn_relay();

    let mut client = build_client(autorelay::Config::default());
    client.dial(relay_a_addr).unwrap();
    client.dial(relay_b_addr).unwrap();

    let mut conns: HashMap<PeerId, ConnectionId> = HashMap::new();
    let mut reserved: HashSet<PeerId> = HashSet::new();
    let mut sleep = futures_timer::Delay::new(Duration::from_secs(20));

    loop {
        tokio::select! {
            _ = &mut sleep => panic!("did not get both reservations in time"),
            ev = client.select_next_some() => match ev {
                SwarmEvent::ConnectionEstablished {
                    peer_id, connection_id, endpoint, ..
                } if !endpoint.is_relayed()
                    && (peer_id == relay_a_peer || peer_id == relay_b_peer) =>
                {
                    conns.insert(peer_id, connection_id);
                }
                SwarmEvent::Behaviour(ClientEvent::RelayClient(
                    relay::client::Event::ReservationReqAccepted { relay_peer_id, .. }
                )) if relay_peer_id == relay_a_peer || relay_peer_id == relay_b_peer => {
                    reserved.insert(relay_peer_id);
                }
                _ => {}
            }
        }
        if reserved.len() == 2 {
            break;
        }
    }

    let conn_a = *conns.get(&relay_a_peer).unwrap();
    let conn_b = *conns.get(&relay_b_peer).unwrap();

    assert!(client.close_connection(conn_a));
    assert!(client.close_connection(conn_b));

    let mut starved_count = 0usize;
    let mut sleep = futures_timer::Delay::new(Duration::from_secs(5));

    loop {
        tokio::select! {
            _ = &mut sleep => break,
            ev = client.select_next_some() => {
                if matches!(
                    ev,
                    SwarmEvent::Behaviour(ClientEvent::Autorelay(
                        autorelay::Event::NoRelaysAvailable
                    ))
                ) {
                    starved_count += 1;
                }
            }
        }
    }

    assert_eq!(
        starved_count, 1,
        "NoRelaysAvailable should fire exactly once across multiple meet_reservation_target invocations"
    );
}

#[tokio::test]
async fn autorelay_resumes_after_public_address_removed() {
    init_tracing();

    let (relay_peer, relay_addr) = spawn_relay();

    let mut client = build_client(autorelay::Config::default());
    client.dial(relay_addr).unwrap();

    wait_for_reservation_from(&mut client, relay_peer, Duration::from_secs(15)).await;

    let public_addr = memory_addr();
    client.add_external_address(public_addr.clone());

    wait_until(&mut client, Duration::from_secs(10), |event| {
        matches!(event, SwarmEvent::ExternalAddrExpired { .. })
    })
    .await;

    client.remove_external_address(&public_addr);

    wait_for_reservation_from(&mut client, relay_peer, Duration::from_secs(15)).await;
}

#[tokio::test]
async fn autorelay_manual_enable_ignores_public_address() {
    init_tracing();

    let (relay_peer, relay_addr) = spawn_relay();

    let mut client = build_client(autorelay::Config::default());
    client
        .behaviour_mut()
        .autorelay
        .set_status(Some(autorelay::Status::Enable));
    client.dial(relay_addr).unwrap();

    wait_for_reservation_from(&mut client, relay_peer, Duration::from_secs(15)).await;

    client.add_external_address(memory_addr());

    let mut sleep = futures_timer::Delay::new(Duration::from_secs(3));

    loop {
        tokio::select! {
            _ = &mut sleep => break,
            ev = client.select_next_some() => {
                if let SwarmEvent::ListenerClosed { reason: Err(_), .. } = ev {
                    panic!("manual-Enable autorelay dropped reservation after public addr appeared");
                }
                if let SwarmEvent::ExternalAddrExpired { address } = &ev
                    && address.iter().any(|p| p == Protocol::P2pCircuit)
                {
                    panic!("manual-Enable autorelay expired the relayed external address");
                }
                if let SwarmEvent::Behaviour(ClientEvent::Autorelay(
                    autorelay::Event::StatusChanged { status: autorelay::Status::Disable },
                )) = ev
                {
                    panic!("manual-Enable autorelay flipped to Disable on public addr");
                }
            }
        }
    }
}

#[tokio::test]
async fn autorelay_forgets_previous_relay_on_reacquire() {
    init_tracing();

    let (relay_peer, relay_addr) = spawn_relay();

    let mut client = build_client(autorelay::Config::default());
    client.dial(relay_addr.clone()).unwrap();

    let conn_id =
        wait_for_reservation_with_conn(&mut client, relay_peer, Duration::from_secs(15)).await;

    assert!(client.close_connection(conn_id));

    wait_until(&mut client, Duration::from_secs(10), |event| {
        matches!(
            event,
            SwarmEvent::Behaviour(ClientEvent::Autorelay(autorelay::Event::NoRelaysAvailable))
        )
    })
    .await;

    assert!(
        client
            .behaviour()
            .autorelay
            .previous_relays()
            .any(|(p, _, _)| *p == relay_peer),
        "expected {relay_peer} in previous_relays after loss"
    );

    client.dial(relay_addr).unwrap();

    wait_until(&mut client, Duration::from_secs(15), |event| {
        matches!(
            event,
            SwarmEvent::NewListenAddr { address, .. } if address.iter().any(|p| p == Protocol::P2pCircuit)
        )
    })
    .await;

    let previous: Vec<PeerId> = client
        .behaviour()
        .autorelay
        .previous_relays()
        .map(|(p, _, _)| *p)
        .collect();
    assert!(
        !previous.contains(&relay_peer),
        "expected {relay_peer} to be removed from previous_relays after re-acquire, got {previous:?}"
    );
}

#[tokio::test]
async fn autorelay_previous_relays_is_bounded() {
    init_tracing();

    let peers_and_addrs: Vec<(PeerId, Multiaddr)> = (0..3).map(|_| spawn_relay()).collect();

    let mut client = build_client(
        autorelay::Config::default()
            .set_max_reservations(NonZeroU8::new(1).unwrap())
            .set_max_previous_relays(2),
    );

    for (peer, addr) in &peers_and_addrs {
        client.dial(addr.clone()).unwrap();

        let conn_id =
            wait_for_reservation_with_conn(&mut client, *peer, Duration::from_secs(15)).await;

        assert!(client.close_connection(conn_id));

        wait_until(&mut client, Duration::from_secs(10), |event| {
            matches!(
                event,
                SwarmEvent::Behaviour(ClientEvent::Autorelay(autorelay::Event::NoRelaysAvailable))
            )
        })
        .await;
    }

    let previous: Vec<PeerId> = client
        .behaviour()
        .autorelay
        .previous_relays()
        .map(|(p, _, _)| *p)
        .collect();

    assert_eq!(
        previous.len(),
        2,
        "expected previous_relays to be bounded to 2, got {previous:?}"
    );
    assert!(
        !previous.contains(&peers_and_addrs[0].0),
        "oldest relay should have been evicted: {previous:?}"
    );
    assert!(previous.contains(&peers_and_addrs[1].0));
    assert!(previous.contains(&peers_and_addrs[2].0));
}

#[tokio::test]
async fn autorelay_static_relay_dial_cooldown_after_failure() {
    init_tracing();

    let cooldown = Duration::from_secs(2);
    let mut client = build_client(autorelay::Config::default().set_failure_cooldown(cooldown));

    let unreachable_peer = PeerId::random();
    let unreachable_addr = memory_addr();

    client
        .behaviour_mut()
        .autorelay
        .add_static_relay(unreachable_peer, unreachable_addr.clone());

    wait_until(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            SwarmEvent::OutgoingConnectionError { peer_id: Some(p), .. } if *p == unreachable_peer
        )
    })
    .await;

    let first_failure_at = std::time::Instant::now();

    client
        .behaviour_mut()
        .autorelay
        .add_static_relay(unreachable_peer, unreachable_addr.clone());

    let mut redialed = false;
    let mut watch = futures_timer::Delay::new(cooldown / 2);
    loop {
        tokio::select! {
            _ = &mut watch => break,
            ev = client.select_next_some() => {
                if matches!(
                    ev,
                    SwarmEvent::OutgoingConnectionError { peer_id: Some(p), .. } if p == unreachable_peer
                ) {
                    redialed = true;
                    break;
                }
            }
        }
    }
    assert!(!redialed, "autorelay redialed within cooldown");

    let remaining = cooldown
        .checked_sub(first_failure_at.elapsed())
        .unwrap_or_default();
    if !remaining.is_zero() {
        futures_timer::Delay::new(remaining + Duration::from_millis(200)).await;
    }

    client
        .behaviour_mut()
        .autorelay
        .add_static_relay(unreachable_peer, unreachable_addr);

    wait_until(&mut client, Duration::from_secs(5), |event| {
        matches!(
            event,
            SwarmEvent::OutgoingConnectionError { peer_id: Some(p), .. } if *p == unreachable_peer
        )
    })
    .await;
}

#[tokio::test]
async fn autorelay_evicts_discovered_peers_for_static() {
    init_tracing();

    let (opp_a_peer, opp_a_addr) = spawn_relay();
    let (opp_b_peer, opp_b_addr) = spawn_relay();
    let (static_peer, static_addr) = spawn_relay();

    let mut client =
        build_client(autorelay::Config::default().set_max_reservations(NonZeroU8::new(1).unwrap()));

    client.dial(opp_a_addr).unwrap();
    client.dial(opp_b_addr).unwrap();

    wait_until_some(&mut client, Duration::from_secs(20), |event| {
        if let SwarmEvent::Behaviour(ClientEvent::RelayClient(
            relay::client::Event::ReservationReqAccepted { relay_peer_id, .. },
        )) = event
            && (*relay_peer_id == opp_a_peer || *relay_peer_id == opp_b_peer)
        {
            Some(*relay_peer_id)
        } else {
            None
        }
    })
    .await;

    client
        .behaviour_mut()
        .autorelay
        .add_static_relay(static_peer, static_addr);

    wait_for_reservation_from(&mut client, static_peer, Duration::from_secs(20)).await;
}

async fn wait_until<F>(client: &mut Swarm<Client>, timeout: Duration, mut predicate: F)
where
    F: FnMut(&SwarmEvent<ClientEvent>) -> bool,
{
    let mut sleep = futures_timer::Delay::new(timeout);
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

async fn with_timeout<F: Future>(future: F, timeout: Duration) -> Option<F::Output> {
    use futures::future::Either;
    let timer = futures_timer::Delay::new(timeout);
    futures::pin_mut!(future);
    match futures::future::select(future, timer).await {
        Either::Left((output, _)) => Some(output),
        Either::Right(_) => None,
    }
}

async fn wait_for_reservation_from(client: &mut Swarm<Client>, peer: PeerId, timeout: Duration) {
    wait_until(client, timeout, |event| {
        matches!(
            event,
            SwarmEvent::Behaviour(ClientEvent::RelayClient(
                relay::client::Event::ReservationReqAccepted { relay_peer_id, .. }
            )) if *relay_peer_id == peer
        )
    })
    .await;
}

async fn wait_for_reservation_with_conn(
    client: &mut Swarm<Client>,
    peer: PeerId,
    timeout: Duration,
) -> ConnectionId {
    wait_until_some(client, timeout, {
        let mut established: Option<ConnectionId> = None;
        let mut reserved = false;
        move |event| {
            match event {
                SwarmEvent::ConnectionEstablished {
                    peer_id,
                    connection_id,
                    endpoint,
                    ..
                } if *peer_id == peer && !endpoint.is_relayed() => {
                    established = Some(*connection_id);
                }
                SwarmEvent::Behaviour(ClientEvent::RelayClient(
                    relay::client::Event::ReservationReqAccepted { relay_peer_id, .. },
                )) if *relay_peer_id == peer => {
                    reserved = true;
                }
                _ => {}
            }
            if reserved { established } else { None }
        }
    })
    .await
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
}

fn memory_addr() -> Multiaddr {
    Multiaddr::empty().with(Protocol::Memory(rand::random::<u64>()))
}

fn spawn_relay() -> (PeerId, Multiaddr) {
    spawn_relay_swarm(build_relay())
}

fn spawn_rejecting_relay() -> (PeerId, Multiaddr) {
    spawn_relay_swarm(build_rejecting_relay())
}

fn spawn_relay_swarm(mut relay: Swarm<Relay>) -> (PeerId, Multiaddr) {
    let addr = memory_addr();
    let peer = *relay.local_peer_id();
    relay.listen_on(addr.clone()).unwrap();
    relay.add_external_address(addr.clone());
    tokio::spawn(relay.collect::<Vec<_>>());
    (peer, addr)
}

fn build_relay() -> Swarm<Relay> {
    build_relay_with_config(relay::Config {
        reservation_duration: Duration::from_secs(60),
        ..Default::default()
    })
}

fn build_rejecting_relay() -> Swarm<Relay> {
    build_relay_with_config(relay::Config {
        max_reservations: 0,
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
            relay: relay::Behaviour::new(local_peer_id, config),
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
