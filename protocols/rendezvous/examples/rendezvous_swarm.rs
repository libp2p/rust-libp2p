use futures::executor::block_on;
use futures::{future, StreamExt};
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::upgrade::{SelectUpgrade, Version};
use libp2p_core::PeerId;
use libp2p_core::{identity, Transport};
use libp2p_mplex::MplexConfig;
use libp2p_noise::{Keypair, X25519Spec};
use libp2p_rendezvous::behaviour::{Difficulty, Rendezvous};
use libp2p_swarm::Swarm;
use libp2p_tcp::TcpConfig;
use libp2p_yamux::YamuxConfig;
use std::task::Poll;
use std::time::Duration;

fn main() {
    let identity = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(identity.public());

    let dh_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&identity)
        .expect("failed to create dh_keys");
    let noise_config = libp2p_noise::NoiseConfig::xx(dh_keys).into_authenticated();

    let tcp_config = TcpConfig::new();
    let transport = tcp_config
        .upgrade(Version::V1)
        .authenticate(noise_config)
        .multiplex(SelectUpgrade::new(
            YamuxConfig::default(),
            MplexConfig::new(),
        ))
        .timeout(Duration::from_secs(20))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    let difficulty = Difficulty::from_u32(1).unwrap();
    let behaviour = Rendezvous::new(identity, 1000, difficulty);

    let mut swarm = Swarm::new(transport, behaviour, peer_id);

    println!("peer id: {}", swarm.local_peer_id());

    swarm
        .listen_on("/ip4/0.0.0.0/tcp/62649".parse().unwrap())
        .unwrap();

    let mut listening = false;
    block_on(future::poll_fn(move |cx| loop {
        match swarm.poll_next_unpin(cx) {
            Poll::Ready(Some(event)) => println!("{:?}", event),
            Poll::Ready(None) => return Poll::Ready(()),
            Poll::Pending => {
                if !listening {
                    for addr in Swarm::listeners(&swarm) {
                        println!("Listening on {}", addr);
                        listening = true;
                    }
                }
                return Poll::Pending;
            }
        }
    }));
}
