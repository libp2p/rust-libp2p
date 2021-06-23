use futures::StreamExt;
use libp2p::core::identity;
use libp2p::core::PeerId;
use libp2p::identify::{Identify, IdentifyConfig, IdentifyEvent};
use libp2p::ping::{Ping, PingEvent, PingSuccess};
use libp2p::rendezvous::Rendezvous;
use libp2p::swarm::Swarm;
use libp2p::swarm::SwarmEvent;
use libp2p::NetworkBehaviour;
use libp2p::{development_transport, rendezvous};
use std::str::FromStr;

#[async_std::main]
async fn main() {
    let rendezvous_point =
        PeerId::from_str("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN").unwrap();

    let identity = identity::Keypair::generate_ed25519();

    let mut swarm = Swarm::new(
        development_transport(identity.clone()).await.unwrap(),
        MyBehaviour {
            identify: Identify::new(IdentifyConfig::new(
                "rendezvous-example/1.0.0".to_string(),
                identity.public(),
            )),
            rendezvous: Rendezvous::new(identity.clone(), 10000),
            ping: Ping::default(),
        },
        PeerId::from(identity.public()),
    );

    let _ = swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap());

    swarm
        .dial_addr("/ip4/127.0.0.1/tcp/62649".parse().unwrap())
        .unwrap();

    while let Some(event) = swarm.next().await {
        match event {
            SwarmEvent::NewListenAddr(addr) => {
                println!("Listening on {}", addr);
            }
            // once `/identify` did its job, we know our external address and can register
            SwarmEvent::Behaviour(MyEvent::Identify(IdentifyEvent::Received { .. })) => {
                swarm
                    .behaviour_mut()
                    .rendezvous
                    .register("rendezvous".to_string(), rendezvous_point, None)
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
