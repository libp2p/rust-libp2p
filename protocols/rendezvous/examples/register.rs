use futures::StreamExt;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::upgrade::{SelectUpgrade, Version};
use libp2p::core::PeerId;
use libp2p::core::{identity, Transport};
use libp2p::identify::{Identify, IdentifyConfig, IdentifyEvent};
use libp2p::mplex::MplexConfig;
use libp2p::noise::{Keypair, NoiseConfig, X25519Spec};
use libp2p::ping::{Ping, PingEvent, PingSuccess};
use libp2p::rendezvous;
use libp2p::rendezvous::Rendezvous;
use libp2p::swarm::Swarm;
use libp2p::swarm::SwarmEvent;
use libp2p::tcp::TcpConfig;
use libp2p::yamux::YamuxConfig;
use libp2p::NetworkBehaviour;
use std::str::FromStr;
use std::time::Duration;

#[async_std::main]
async fn main() {
    let identity = identity::Keypair::generate_ed25519();

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
            identify: Identify::new(IdentifyConfig::new(
                "rendezvous-example/1.0.0".to_string(),
                identity.public(),
            )),
            rendezvous: Rendezvous::new(identity, 10000),
            ping: Ping::default(),
        },
        local_peer_id,
    );

    let _ = swarm.listen_on("/ip4/127.0.0.1/tcp/62343".parse().unwrap());

    swarm
        .dial_addr("/ip4/127.0.0.1/tcp/62649".parse().unwrap())
        .unwrap();

    let server_peer_id =
        PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap();

    while let Some(event) = swarm.next().await {
        match event {
            // once `/identify` did its job, we know our external address and can register
            SwarmEvent::Behaviour(MyEvent::Identify(IdentifyEvent::Received { .. })) => {
                swarm
                    .behaviour_mut()
                    .rendezvous
                    .register("rendezvous".to_string(), server_peer_id, None)
                    .unwrap();
            }
            SwarmEvent::Behaviour(MyEvent::Rendezvous(rendezvous::Event::Registered {
                namespace,
                ttl,
                rendezvous_node,
            })) => {
                println!(
                    "Registered for namespace '{}' at rendezvous point {} for the next {} seconds",
                    namespace, rendezvous_node, ttl
                );
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
    Identify(IdentifyEvent),
    Ping(PingEvent),
}

impl From<rendezvous::Event> for MyEvent {
    fn from(event: rendezvous::Event) -> Self {
        MyEvent::Rendezvous(event)
    }
}

impl From<IdentifyEvent> for MyEvent {
    fn from(event: IdentifyEvent) -> Self {
        MyEvent::Identify(event)
    }
}

impl From<PingEvent> for MyEvent {
    fn from(event: PingEvent) -> Self {
        MyEvent::Ping(event)
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(event_process = false)]
#[behaviour(out_event = "MyEvent")]
struct MyBehaviour {
    identify: Identify,
    rendezvous: Rendezvous,
    ping: Ping,
}
