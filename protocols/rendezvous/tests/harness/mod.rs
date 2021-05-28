use futures::future;
use libp2p_core::{Executor, Multiaddr, PeerId, identity, Transport};
use std::pin::Pin;
use futures::Future;
use libp2p_swarm::{NetworkBehaviour, Swarm, IntoProtocolsHandler, ProtocolsHandler, SwarmBuilder, SwarmEvent};
use std::fmt::Debug;
use libp2p_core::transport::MemoryTransport;
use libp2p_core::transport::upgrade::Version;
use libp2p_core::upgrade::SelectUpgrade;
use std::time::Duration;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_noise::{self, NoiseConfig, X25519Spec, Keypair};
use libp2p_yamux::YamuxConfig;
use libp2p_mplex::MplexConfig;

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

pub async fn new_connected_swarm_pair<B, F>(behaviour_fn: F) -> (Actor<B>, Actor<B>)
where
    B: NetworkBehaviour,
    F: Fn(PeerId, identity::Keypair) -> B + Clone,
    <<<B as NetworkBehaviour>::ProtocolsHandler as IntoProtocolsHandler>::Handler as ProtocolsHandler>::InEvent: Clone,
<B as NetworkBehaviour>::OutEvent: Debug{
    let (swarm, addr, peer_id) = new_swarm(behaviour_fn.clone());
    let mut alice = Actor {
        swarm,
        addr,
        peer_id,
    };

    let (swarm, addr, peer_id) = new_swarm(behaviour_fn);
    let mut bob = Actor {
        swarm,
        addr,
        peer_id,
    };

    connect(&mut alice.swarm, &mut bob.swarm).await;

    (alice, bob)
}

pub fn new_swarm<B: NetworkBehaviour, F: Fn(PeerId, identity::Keypair) -> B>(
    behaviour_fn: F,
) -> (Swarm<B>, Multiaddr, PeerId)
where
    B: NetworkBehaviour,
{
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());

    let dh_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&id_keys)
        .expect("failed to create dh_keys");
    let noise = NoiseConfig::xx(dh_keys).into_authenticated();

    let transport = MemoryTransport::default()
        .upgrade(Version::V1)
        .authenticate(noise)
        .multiplex(SelectUpgrade::new(
            YamuxConfig::default(),
            MplexConfig::new(),
        ))
        .timeout(Duration::from_secs(5))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    let mut swarm: Swarm<B> = SwarmBuilder::new(transport, behaviour_fn(peer_id, id_keys), peer_id)
        .executor(Box::new(GlobalSpawnTokioExecutor))
        .build();

    let address_port = rand::random::<u64>();
    let addr = format!("/memory/{}", address_port)
        .parse::<Multiaddr>()
        .unwrap();

    Swarm::listen_on(&mut swarm, addr.clone()).unwrap();

    (swarm, addr, peer_id)
}

pub async fn await_events_or_timeout<A, B>(
    alice_event: impl Future<Output = A>,
    bob_event: impl Future<Output = B>,
) -> (A, B) {
    tokio::time::timeout(
        Duration::from_secs(10),
        future::join(alice_event, bob_event),
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
pub async fn connect<BA, BB>(alice: &mut Swarm<BA>, bob: &mut Swarm<BB>)
where
    BA: NetworkBehaviour,
    BB: NetworkBehaviour,
    <BA as NetworkBehaviour>::OutEvent: Debug,
    <BB as NetworkBehaviour>::OutEvent: Debug,
{
    let mut alice_connected = false;
    let mut bob_connected = false;

    while !alice_connected && !bob_connected {
        let (alice_event, bob_event) = future::join(alice.next_event(), bob.next_event()).await;

        match alice_event {
            SwarmEvent::ConnectionEstablished { .. } => {
                alice_connected = true;
            }
            SwarmEvent::NewListenAddr(addr) => {
                bob.dial_addr(addr).unwrap();
            }
            SwarmEvent::Behaviour(event) => {
                panic!(
                    "alice unexpectedly emitted a behaviour event during connection: {:?}",
                    event
                );
            }
            _ => {}
        }
        match bob_event {
            SwarmEvent::ConnectionEstablished { .. } => {
                bob_connected = true;
            }
            SwarmEvent::Behaviour(event) => {
                panic!(
                    "bob unexpectedly emitted a behaviour event during connection: {:?}",
                    event
                );
            }
            _ => {}
        }
    }
}
