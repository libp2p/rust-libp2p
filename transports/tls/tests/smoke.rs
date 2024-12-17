use futures::{future, StreamExt};
use libp2p_core::{multiaddr::Protocol, transport::MemoryTransport, upgrade::Version, Transport};
use libp2p_swarm::{dummy, Config, Swarm, SwarmEvent};

#[tokio::test]
async fn can_establish_connection() {
    let mut swarm1 = make_swarm();
    let mut swarm2 = make_swarm();

    let listen_address = {
        let expected_listener_id = swarm1.listen_on(Protocol::Memory(0).into()).unwrap();

        loop {
            match swarm1.next().await.unwrap() {
                SwarmEvent::NewListenAddr {
                    address,
                    listener_id,
                } if listener_id == expected_listener_id => break address,
                _ => continue,
            };
        }
    };
    swarm2.dial(listen_address).unwrap();

    let await_inbound_connection = async {
        loop {
            match swarm1.next().await.unwrap() {
                SwarmEvent::ConnectionEstablished { peer_id, .. } => break peer_id,
                SwarmEvent::IncomingConnectionError { error, .. } => {
                    panic!("Incoming connection failed: {error}")
                }
                _ => continue,
            };
        }
    };
    let await_outbound_connection = async {
        loop {
            match swarm2.next().await.unwrap() {
                SwarmEvent::ConnectionEstablished { peer_id, .. } => break peer_id,
                SwarmEvent::OutgoingConnectionError { error, .. } => {
                    panic!("Failed to dial: {error}")
                }
                _ => continue,
            };
        }
    };

    let (inbound_peer_id, outbound_peer_id) =
        future::join(await_inbound_connection, await_outbound_connection).await;

    assert_eq!(&inbound_peer_id, swarm2.local_peer_id());
    assert_eq!(&outbound_peer_id, swarm1.local_peer_id());
}

fn make_swarm() -> Swarm<dummy::Behaviour> {
    let identity = libp2p_identity::Keypair::generate_ed25519();

    let transport = MemoryTransport::default()
        .upgrade(Version::V1)
        .authenticate(libp2p_tls::Config::new(&identity).unwrap())
        .multiplex(libp2p_yamux::Config::default())
        .boxed();

    Swarm::new(
        transport,
        dummy::Behaviour,
        identity.public().to_peer_id(),
        Config::with_tokio_executor(),
    )
}
