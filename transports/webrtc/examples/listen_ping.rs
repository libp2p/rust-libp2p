use anyhow::Result;
use futures::StreamExt;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::Transport;
use libp2p_identity as identity;
use libp2p_ping as ping;
use libp2p_swarm::{Swarm, SwarmBuilder};
use rand::thread_rng;
use std::time::Duration;
use void::Void;

/// An example WebRTC server that will accept connections and run the ping protocol on them.
#[tokio::main]
async fn main() -> Result<()> {
    let mut swarm = create_swarm()?;

    swarm.listen_on("/ip4/127.0.0.1/udp/0/webrtc-direct".parse()?)?;

    loop {
        let event = swarm.next().await.unwrap();
        eprintln!("New event: {event:?}")
    }
}

fn create_swarm() -> Result<Swarm<ping::Behaviour>> {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = id_keys.public().to_peer_id();
    let transport = libp2p_webrtc::tokio::Transport::new(
        id_keys,
        libp2p_webrtc::tokio::Certificate::generate(&mut thread_rng())?,
    );

    let cfg = ping::Config::new().with_interval(Duration::from_millis(10));
    let transport = transport
        .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn)))
        .boxed();

    Ok(
        SwarmBuilder::with_tokio_executor(transport, ping::Behaviour::new(cfg.clone()), peer_id)
            .idle_connection_timeout(Duration::from_secs(5))
            .build(),
    )
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum Event {
    Ping(ping::Event),
}

impl From<ping::Event> for Event {
    fn from(e: ping::Event) -> Self {
        Event::Ping(e)
    }
}

impl From<Void> for Event {
    fn from(event: Void) -> Self {
        void::unreachable(event)
    }
}
