use std::time::Duration;

use futures::{future, AsyncRead, AsyncWrite, StreamExt};
use libp2p_core::transport::MemoryTransport;
use libp2p_core::upgrade::Version;
use libp2p_core::Transport;
use libp2p_core::{multiaddr::Protocol, Multiaddr};
use libp2p_pnet::{PnetConfig, PreSharedKey};
use libp2p_swarm::{keep_alive, NetworkBehaviour, Swarm, SwarmEvent};

const TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn can_establish_connection_memory() {
    can_establish_connection_inner_with_timeout(
        MemoryTransport::default,
        Protocol::Memory(0).into(),
    )
    .await
}

#[tokio::test]
async fn can_establish_connection_tcp() {
    can_establish_connection_inner_with_timeout(
        libp2p_tcp::tokio::Transport::default,
        "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
    )
    .await
}

#[tokio::test]
async fn can_establish_connection_websocket() {
    can_establish_connection_inner_with_timeout(
        || libp2p_websocket::WsConfig::new(libp2p_tcp::tokio::Transport::default()),
        "/ip4/127.0.0.1/tcp/0/ws".parse().unwrap(),
    )
    .await
}

async fn can_establish_connection_inner_with_timeout<F, T>(
    build_transport: F,
    listen_addr: Multiaddr,
) where
    F: Fn() -> T,
    T: Transport + Send + Unpin + 'static,
    <T as libp2p_core::Transport>::Error: Send + Sync + 'static,
    <T as libp2p_core::Transport>::Output: AsyncRead + AsyncWrite + Send + Unpin,
    <T as libp2p_core::Transport>::ListenerUpgrade: Send,
    <T as libp2p_core::Transport>::Dial: Send,
{
    let task = can_establish_connection_inner(build_transport, listen_addr);
    tokio::time::timeout(TIMEOUT, task).await.unwrap();
}

async fn can_establish_connection_inner<F, T>(build_transport: F, listen_addr: Multiaddr)
where
    F: Fn() -> T,
    T: Transport + Send + Unpin + 'static,
    <T as libp2p_core::Transport>::Error: Send + Sync + 'static,
    <T as libp2p_core::Transport>::Output: AsyncRead + AsyncWrite + Send + Unpin,
    <T as libp2p_core::Transport>::ListenerUpgrade: Send,
    <T as libp2p_core::Transport>::Dial: Send,
{
    let pnet = PnetConfig::new(PreSharedKey::new([0; 32]));

    let mut swarm1 = make_swarm(build_transport(), pnet);
    let mut swarm2 = make_swarm(build_transport(), pnet);

    let listen_address = listen_on(&mut swarm1, listen_addr).await;
    swarm2.dial(listen_address).unwrap();
    let await_inbound_connection = async {
        loop {
            match swarm1.select_next_some().await {
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
            match swarm2.select_next_some().await {
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

fn make_swarm<T>(transport: T, pnet: PnetConfig) -> Swarm<keep_alive::Behaviour>
where
    T: Transport + Send + Unpin + 'static,
    <T as libp2p_core::Transport>::Error: Send + Sync + 'static,
    <T as libp2p_core::Transport>::Output: AsyncRead + AsyncWrite + Send + Unpin,
    <T as libp2p_core::Transport>::ListenerUpgrade: Send,
    <T as libp2p_core::Transport>::Dial: Send,
{
    let identity = libp2p_identity::Keypair::generate_ed25519();
    let transport = transport
        .and_then(move |socket, _| pnet.handshake(socket))
        .upgrade(Version::V1)
        .authenticate(libp2p_noise::NoiseAuthenticated::xx(&identity).unwrap())
        .multiplex(libp2p_yamux::YamuxConfig::default())
        .boxed();
    Swarm::with_tokio_executor(
        transport,
        keep_alive::Behaviour,
        identity.public().to_peer_id(),
    )
}

async fn listen_on<B: NetworkBehaviour>(swarm: &mut Swarm<B>, addr: Multiaddr) -> Multiaddr {
    let expected_listener_id = swarm.listen_on(addr).unwrap();
    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr {
                address,
                listener_id,
            } if listener_id == expected_listener_id => break address,
            _ => continue,
        };
    }
}
