//! Integration tests for the [`Config::with_relay_for_requests`] policy.
//!
//! The setup mirrors the relay/DCUtR test harness: a relay server, a `server` that holds a
//! reservation on the relay (reachable via a relayed address) and a `client` that talks to it.

#![cfg(feature = "cbor")]

use std::time::Duration;

use futures::StreamExt;
use libp2p_core::{
    Multiaddr,
    multiaddr::Protocol,
    transport::{MemoryTransport, Transport, upgrade::Version},
};
use libp2p_identify as identify;
use libp2p_identity as identity;
use libp2p_identity::PeerId;
use libp2p_relay as relay;
use libp2p_request_response::{self as request_response, ProtocolSupport};
use libp2p_swarm::{Config as SwarmConfig, NetworkBehaviour, StreamProtocol, Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt as _;
use serde::{Deserialize, Serialize};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Ping(Vec<u8>);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Pong(Vec<u8>);

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct Relay {
    relay: relay::Behaviour,
    identify: identify::Behaviour,
}

#[derive(NetworkBehaviour)]
#[behaviour(prelude = "libp2p_swarm::derive_prelude")]
struct Client {
    relay: relay::client::Behaviour,
    request_response: request_response::cbor::Behaviour<Ping, Pong>,
    identify: identify::Behaviour,
}

/// A relayed-only request succeeds with the default configuration.
#[tokio::test]
async fn request_over_relay_succeeds_by_default() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let (relay_peer_id, relay_tcp_addr) = spawn_relay().await;

    let server = build_client(request_response::Config::default(), LONG_IDLE);
    let server_peer_id = *server.local_peer_id();
    let server_relayed_addr = relayed_addr(&relay_tcp_addr, relay_peer_id, server_peer_id);
    reserve_and_spawn_server(server, server_relayed_addr.clone(), relay_peer_id).await;

    let mut client = build_client(request_response::Config::default(), LONG_IDLE);
    client.dial_and_wait(server_relayed_addr).await;
    client
        .behaviour_mut()
        .request_response
        .send_request(&server_peer_id, Ping(b"ping".to_vec()));

    let pong = client
        .wait(|event| match event {
            SwarmEvent::Behaviour(ClientEvent::RequestResponse(
                request_response::Event::Message {
                    message: request_response::Message::Response { response, .. },
                    ..
                },
            )) => Some(response),
            SwarmEvent::Behaviour(ClientEvent::RequestResponse(
                request_response::Event::OutboundFailure { error, .. },
            )) => panic!("unexpected outbound failure: {error}"),
            _ => None,
        })
        .await;

    assert_eq!(pong, Pong(b"ping".to_vec()));
}

/// With relayed requests disabled and only a relayed connection available, the request is kept
/// queued (not sent over the relay, not failed) and only fails with
/// [`request_response::OutboundFailure::NoDirectConnection`] once the peer fully disconnects.
#[tokio::test]
async fn request_over_relay_is_queued_then_fails_on_disconnect() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let (relay_peer_id, relay_tcp_addr) = spawn_relay().await;

    let server = build_client(request_response::Config::default(), LONG_IDLE);
    let server_peer_id = *server.local_peer_id();
    let server_relayed_addr = relayed_addr(&relay_tcp_addr, relay_peer_id, server_peer_id);
    reserve_and_spawn_server(server, server_relayed_addr.clone(), relay_peer_id).await;

    // Short idle timeout so the unused relayed connection is closed by the swarm shortly after the
    // request is queued (this is the mechanism that bounds how long a request can stay queued).
    let mut client = build_client(
        request_response::Config::default().with_relay_for_requests(false),
        Duration::from_millis(500),
    );
    client.dial_and_wait(server_relayed_addr).await;
    let request_id = client
        .behaviour_mut()
        .request_response
        .send_request(&server_peer_id, Ping(b"ping".to_vec()));

    // The request is neither sent over the relay nor failed yet: it stays queued.
    assert!(
        client
            .behaviour()
            .request_response
            .is_pending_outbound(&server_peer_id, &request_id),
        "request should be queued while only a relayed connection is available"
    );

    // Once the idle relayed connection is closed, the still-queued request fails.
    let error = client
        .wait(|event| match event {
            SwarmEvent::Behaviour(ClientEvent::RequestResponse(
                request_response::Event::OutboundFailure { error, .. },
            )) => Some(error),
            SwarmEvent::Behaviour(ClientEvent::RequestResponse(
                request_response::Event::Message { .. },
            )) => panic!("request was unexpectedly delivered over the relay"),
            _ => None,
        })
        .await;

    assert!(
        matches!(error, request_response::OutboundFailure::NoDirectConnection),
        "expected NoDirectConnection, got {error:?}"
    );
}

/// With relayed requests disabled, a request sent while only a relayed connection exists stays
/// queued and is delivered once a direct connection is established (modelling a DCUtR upgrade). The
/// server asserts that the request did not arrive over a relay.
#[tokio::test]
async fn queued_request_is_delivered_after_direct_upgrade() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let (relay_peer_id, relay_tcp_addr) = spawn_relay().await;

    let mut server = build_client(request_response::Config::default(), LONG_IDLE);
    let server_peer_id = *server.local_peer_id();
    let server_relayed_addr = relayed_addr(&relay_tcp_addr, relay_peer_id, server_peer_id);
    let (_, server_direct_addr) = server.listen().await;
    let server_direct_addr = server_direct_addr.with_p2p(server_peer_id).unwrap();

    reserve(&mut server, server_relayed_addr.clone(), relay_peer_id).await;
    tokio::spawn(drive_server(server, true));

    let mut client = build_client(
        request_response::Config::default().with_relay_for_requests(false),
        LONG_IDLE,
    );

    // Only a relayed connection exists when the request is sent: it must stay queued.
    client.dial_and_wait(server_relayed_addr).await;
    let request_id = client
        .behaviour_mut()
        .request_response
        .send_request(&server_peer_id, Ping(b"ping".to_vec()));
    assert!(
        client
            .behaviour()
            .request_response
            .is_pending_outbound(&server_peer_id, &request_id),
    );

    // Establishing a direct connection (the DCUtR upgrade) drains the queue over the direct link.
    client.dial(server_direct_addr).unwrap();

    let pong = client
        .wait(|event| match event {
            SwarmEvent::Behaviour(ClientEvent::RequestResponse(
                request_response::Event::Message {
                    message: request_response::Message::Response { response, .. },
                    ..
                },
            )) => Some(response),
            SwarmEvent::Behaviour(ClientEvent::RequestResponse(
                request_response::Event::OutboundFailure { error, .. },
            )) => panic!("unexpected outbound failure: {error}"),
            _ => None,
        })
        .await;

    assert_eq!(pong, Pong(b"ping".to_vec()));
}

/// A generous idle timeout that keeps relayed connections open for the duration of a test.
const LONG_IDLE: Duration = Duration::from_secs(30);

fn build_client(config: request_response::Config, idle_timeout: Duration) -> Swarm<Client> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = local_key.public().to_peer_id();

    let (relay_transport, relay_behaviour) = relay::client::new(local_peer_id);

    let transport = relay_transport
        .or_transport(MemoryTransport::default())
        .or_transport(libp2p_tcp::tokio::Transport::default())
        .upgrade(Version::V1)
        .authenticate(libp2p_plaintext::Config::new(&local_key))
        .multiplex(libp2p_yamux::Config::default())
        .boxed();

    let protocols = std::iter::once((StreamProtocol::new("/test-rr/1"), ProtocolSupport::Full));

    Swarm::new(
        transport,
        Client {
            relay: relay_behaviour,
            request_response: request_response::cbor::Behaviour::new(protocols, config),
            identify: identify::Behaviour::new(identify::Config::new(
                "/client".to_owned(),
                local_key.public(),
            )),
        },
        local_peer_id,
        SwarmConfig::with_tokio_executor().with_idle_connection_timeout(idle_timeout),
    )
}

async fn spawn_relay() -> (PeerId, Multiaddr) {
    let mut relay = Swarm::new_ephemeral_tokio(|identity| Relay {
        relay: relay::Behaviour::new(identity.public().to_peer_id(), relay::Config::default()),
        identify: identify::Behaviour::new(identify::Config::new(
            "/relay".to_owned(),
            identity.public(),
        )),
    });

    let (_, relay_tcp_addr) = relay.listen().with_tcp_addr_external().await;
    let relay_peer_id = *relay.local_peer_id();
    tokio::spawn(relay.loop_on_next());

    (relay_peer_id, relay_tcp_addr)
}

fn relayed_addr(relay_tcp_addr: &Multiaddr, relay_peer_id: PeerId, peer_id: PeerId) -> Multiaddr {
    relay_tcp_addr
        .clone()
        .with(Protocol::P2p(relay_peer_id))
        .with(Protocol::P2pCircuit)
        .with(Protocol::P2p(peer_id))
}

/// Reserves a slot on the relay and then spawns the server to keep responding/renewing.
async fn reserve_and_spawn_server(
    mut server: Swarm<Client>,
    relayed_addr: Multiaddr,
    relay_peer_id: PeerId,
) {
    reserve(&mut server, relayed_addr, relay_peer_id).await;
    tokio::spawn(drive_server(server, false));
}

/// Drives the server's reservation handshake until its relayed address is usable.
async fn reserve(server: &mut Swarm<Client>, relayed_addr: Multiaddr, relay_peer_id: PeerId) {
    server.listen_on(relayed_addr.clone()).unwrap();

    let mut listening = false;
    let mut reservation_accepted = false;
    let mut address_observed = false;

    while !(listening && reservation_accepted && address_observed) {
        match server.next_swarm_event().await {
            SwarmEvent::NewListenAddr { address, .. } if address == relayed_addr => {
                listening = true;
            }
            SwarmEvent::Behaviour(ClientEvent::Relay(
                relay::client::Event::ReservationReqAccepted {
                    relay_peer_id: peer_id,
                    ..
                },
            )) if peer_id == relay_peer_id => {
                reservation_accepted = true;
            }
            SwarmEvent::Behaviour(ClientEvent::Identify(identify::Event::Received { .. })) => {
                address_observed = true;
            }
            _ => {}
        }
    }
}

/// Responds to every inbound request with a `Pong` echoing the payload. When `require_direct` is
/// set, asserts that requests are never delivered over a relayed connection (tracked via the
/// established connections' endpoints).
async fn drive_server(mut server: Swarm<Client>, require_direct: bool) {
    use std::collections::HashMap;

    let mut relayed_connections: HashMap<_, bool> = HashMap::new();

    loop {
        match server.select_next_some().await {
            SwarmEvent::ConnectionEstablished {
                connection_id,
                endpoint,
                ..
            } => {
                relayed_connections.insert(connection_id, endpoint.is_relayed());
            }
            SwarmEvent::ConnectionClosed { connection_id, .. } => {
                relayed_connections.remove(&connection_id);
            }
            SwarmEvent::Behaviour(ClientEvent::RequestResponse(
                request_response::Event::Message {
                    connection_id,
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                    ..
                },
            )) => {
                if require_direct {
                    assert_eq!(
                        relayed_connections.get(&connection_id),
                        Some(&false),
                        "request was delivered over a relayed connection"
                    );
                }
                let _ = server
                    .behaviour_mut()
                    .request_response
                    .send_response(channel, Pong(request.0));
            }
            _ => {}
        }
    }
}
