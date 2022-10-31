use anyhow::Result;
use futures::StreamExt;
use libp2p::swarm::{keep_alive, SwarmBuilder};
use libp2p::{ping, Swarm};
use libp2p::Transport;
use libp2p_core::identity;
use libp2p_core::muxing::StreamMuxerBox;
use rand::thread_rng;

/// An example WebRTC server that will accept connections and run the ping protocol on them.
#[tokio::main]
async fn main() -> Result<()> {
    let mut swarm = create_swarm()?;

    swarm.listen_on("/ip4/127.0.0.1/udp/0/webrtc".parse()?)?;

    loop {
        let event = swarm.next().await.unwrap();
        eprintln!("New event: {event:?}")
    }
}

fn create_swarm() -> Result<Swarm<Behaviour>> {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = id_keys.public().to_peer_id();
    let transport = libp2p_webrtc::tokio::Transport::new(
        id_keys,
        libp2p_webrtc::tokio::Certificate::generate(&mut thread_rng())?,
    );

    let transport = transport
        .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn)))
        .boxed();

    Ok(
        SwarmBuilder::new(transport, Behaviour::default(), peer_id)
            .executor(Box::new(|fut| {
                tokio::spawn(fut);
            }))
            .build(),
    )
}

#[derive(libp2p::NetworkBehaviour, Default)]
struct Behaviour {
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour
}
