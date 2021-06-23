use futures::StreamExt;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::upgrade::{SelectUpgrade, Version};
use libp2p::core::PeerId;
use libp2p::core::{identity, Transport};
use libp2p::mplex::MplexConfig;
use libp2p::multiaddr::Protocol;
use libp2p::noise::{Keypair, NoiseConfig, X25519Spec};
use libp2p::ping::{Ping, PingEvent, PingSuccess};
use libp2p::rendezvous::Rendezvous;
use libp2p::swarm::Swarm;
use libp2p::swarm::SwarmEvent;
use libp2p::tcp::TcpConfig;
use libp2p::yamux::YamuxConfig;
use libp2p::{rendezvous, Multiaddr};
use std::str::FromStr;
use std::time::Duration;

const NAMESPACE: &'static str = "rendezvous";

#[async_std::main]
async fn main() {
    let identity = identity::Keypair::generate_ed25519();
    let rendezvous_point = "/ip4/127.0.0.1/tcp/62649".parse::<Multiaddr>().unwrap();

    let transport = TcpConfig::new()
        .upgrade(Version::V1)
        .authenticate(
            NoiseConfig::xx(
                Keypair::<X25519Spec>::new()
                    .into_authentic(&identity)
                    .expect("failed to create dh_keys"),
            )
            .into_authenticated(),
        )
        .multiplex(SelectUpgrade::new(
            YamuxConfig::default(),
            MplexConfig::new(),
        ))
        .timeout(Duration::from_secs(20))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .boxed();

    let local_peer_id = PeerId::from(identity.public());
    let mut swarm = Swarm::new(
        transport,
        MyBehaviour {
            rendezvous: Rendezvous::new(identity, 10000),
            ping: Ping::default(),
        },
        local_peer_id,
    );

    let _ = swarm.dial_addr(rendezvous_point.clone());

    let server_peer_id =
        PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap();

    while let Some(event) = swarm.next().await {
        match event {
            SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == server_peer_id => {
                println!(
                    "Connected to rendezvous point, discovering nodes in `{}` namespace ...",
                    NAMESPACE
                );

                swarm.behaviour_mut().rendezvous.discover(
                    Some(NAMESPACE.to_string()),
                    None,
                    None,
                    server_peer_id,
                );
            }
            SwarmEvent::UnreachableAddr { error, address, .. }
            | SwarmEvent::UnknownPeerUnreachableAddr { error, address, .. }
                if address == rendezvous_point =>
            {
                println!(
                    "Failed to connect to rendezvous point at {}: {}",
                    address, error
                );
                return;
            }
            SwarmEvent::Behaviour(MyEvent::Rendezvous(rendezvous::Event::Discovered {
                registrations,
                ..
            })) => {
                for ((_, peer), registration) in registrations {
                    for address in registration.record.addresses() {
                        println!("Discovered peer {} at {}", peer, address);

                        let p2p_suffix = Protocol::P2p(peer.as_ref().clone());
                        let address_with_p2p =
                            if !address.ends_with(&Multiaddr::empty().with(p2p_suffix.clone())) {
                                address.clone().with(p2p_suffix)
                            } else {
                                address.clone()
                            };

                        swarm.dial_addr(address_with_p2p).unwrap()
                    }
                }
            }
            SwarmEvent::Behaviour(MyEvent::Ping(PingEvent {
                peer,
                result: Ok(PingSuccess::Ping { rtt }),
            })) => {
                println!("Ping to {} is {}ms", peer, rtt.as_millis())
            }
            _ => {}
        }
    }
}

#[derive(Debug)]
enum MyEvent {
    Rendezvous(rendezvous::Event),
    Ping(PingEvent),
}

impl From<rendezvous::Event> for MyEvent {
    fn from(event: rendezvous::Event) -> Self {
        MyEvent::Rendezvous(event)
    }
}

impl From<PingEvent> for MyEvent {
    fn from(event: PingEvent) -> Self {
        MyEvent::Ping(event)
    }
}

#[derive(libp2p::NetworkBehaviour)]
#[behaviour(event_process = false)]
#[behaviour(out_event = "MyEvent")]
struct MyBehaviour {
    rendezvous: Rendezvous,
    ping: Ping,
}
