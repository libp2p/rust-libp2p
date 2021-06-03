use futures::future;
use futures::Future;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::transport::upgrade::Version;
use libp2p_core::transport::MemoryTransport;
use libp2p_core::upgrade::{EitherUpgrade, SelectUpgrade};
use libp2p_core::{identity, Executor, Multiaddr, PeerId, Transport};
use libp2p_mplex::MplexConfig;
use libp2p_noise as noise;
use libp2p_noise::{self, Keypair, NoiseConfig, X25519Spec};
use libp2p_swarm::{NetworkBehaviour, Swarm, SwarmBuilder, SwarmEvent};
use libp2p_tcp::TcpConfig;
use libp2p_yamux::YamuxConfig;
use log::debug;
use std::fmt::Debug;
use std::pin::Pin;
use std::time::Duration;

/// An adaptor struct for libp2p that spawns futures into the current
/// thread-local runtime.
struct GlobalSpawnTokioExecutor;

impl Executor for GlobalSpawnTokioExecutor {
    fn exec(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        let _ = tokio::spawn(future);
    }
}

#[allow(missing_debug_implementations)]
pub struct Actor<B: NetworkBehaviour> {
    pub swarm: Swarm<B>,
    pub addr: Multiaddr,
    pub peer_id: PeerId,
}

pub fn new_swarm<B: NetworkBehaviour, F: Fn(PeerId, identity::Keypair) -> B>(
    behaviour_fn: F,
    id_keys: identity::Keypair,
    listen_address: Multiaddr,
) -> (Swarm<B>, Multiaddr, PeerId)
where
    B: NetworkBehaviour,
{
    let peer_id = PeerId::from(id_keys.public());

    let noise_keys = noise::Keypair::<noise::X25519Spec>::new()
        .into_authentic(&id_keys)
        .unwrap();

    let transport = TcpConfig::new()
        .nodelay(true)
        .upgrade(Version::V1)
        .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
        .multiplex(YamuxConfig::default())
        .boxed();

    let mut swarm: Swarm<B> = SwarmBuilder::new(transport, behaviour_fn(peer_id, id_keys), peer_id)
        .executor(Box::new(GlobalSpawnTokioExecutor))
        .build();

    Swarm::listen_on(&mut swarm, listen_address.clone()).unwrap();

    (swarm, listen_address, peer_id)
}

pub async fn await_events_or_timeout<A, B>(
    swarm_1_event: impl Future<Output = A>,
    swarm_2_event: impl Future<Output = B>,
) -> (A, B) {
    tokio::time::timeout(
        Duration::from_secs(10),
        future::join(swarm_1_event, swarm_2_event),
    )
    .await
    .expect("network behaviours to emit an event within 10 seconds")
}

/// Connects two swarms with each other.
///
/// This assumes the transport that is in use can be used by Bob to connect to
/// the listen address that is emitted by Alice. In other words, they have to be
/// on the same network. The memory transport used by the above `new_swarm`
/// function fulfills this.
///
/// We also assume that the swarms don't emit any behaviour events during the
/// connection phase. Any event emitted is considered a bug from this functions
/// PoV because they would be lost.
pub async fn connect<BA, BB>(receiver: &mut Swarm<BA>, dialer: &mut Swarm<BB>)
where
    BA: NetworkBehaviour,
    BB: NetworkBehaviour,
    <BA as NetworkBehaviour>::OutEvent: Debug,
    <BB as NetworkBehaviour>::OutEvent: Debug,
{
    let mut alice_connected = false;
    let mut bob_connected = false;

    while !alice_connected && !bob_connected {
        let (alice_event, bob_event) =
            future::join(receiver.next_event(), dialer.next_event()).await;

        match alice_event {
            SwarmEvent::ConnectionEstablished { .. } => {
                alice_connected = true;
            }
            SwarmEvent::NewListenAddr(addr) => {
                dialer.dial_addr(addr).unwrap();
            }
            SwarmEvent::Behaviour(event) => {
                panic!(
                    "alice unexpectedly emitted a behaviour event during connection: {:?}",
                    event
                );
            }
            _ => {
                debug!("{:?}", alice_event);
            }
        }
        match &bob_event {
            SwarmEvent::ConnectionEstablished { .. } => {
                bob_connected = true;
            }
            SwarmEvent::Behaviour(event) => {
                panic!(
                    "bob unexpectedly emitted a behaviour event during connection: {:?}",
                    event
                );
            }
            _ => {
                debug!("{:?}", bob_event);
            }
        }
    }
}
