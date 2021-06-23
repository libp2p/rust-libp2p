use futures::StreamExt;
use libp2p::core::identity;
use libp2p::core::PeerId;
use libp2p::multiaddr::Protocol;
use libp2p::ping::{Ping, PingEvent, PingSuccess};
use libp2p::rendezvous::Rendezvous;
use libp2p::swarm::Swarm;
use libp2p::swarm::SwarmEvent;
use libp2p::{development_transport, rendezvous, Multiaddr};

const NAMESPACE: &'static str = "rendezvous";

#[async_std::main]
async fn main() {
    env_logger::init();

    let identity = identity::Keypair::generate_ed25519();
    let rendezvous_point_address = "/ip4/127.0.0.1/tcp/62649".parse::<Multiaddr>().unwrap();
    let rendezvous_point = "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
        .parse()
        .unwrap();

    let mut swarm = Swarm::new(
        development_transport(identity.clone()).await.unwrap(),
        MyBehaviour {
            rendezvous: Rendezvous::new(identity.clone(), 10000),
            ping: Ping::default(),
        },
        PeerId::from(identity.public()),
    );

    let _ = swarm.dial_addr(rendezvous_point_address.clone());

    while let Some(event) = swarm.next().await {
        match event {
            SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == rendezvous_point => {
                log::info!(
                    "Connected to rendezvous point, discovering nodes in '{}' namespace ...",
                    NAMESPACE
                );

                swarm.behaviour_mut().rendezvous.discover(
                    Some(NAMESPACE.to_string()),
                    None,
                    None,
                    rendezvous_point,
                );
            }
            SwarmEvent::UnreachableAddr { error, address, .. }
            | SwarmEvent::UnknownPeerUnreachableAddr { error, address, .. }
                if address == rendezvous_point_address =>
            {
                log::error!(
                    "Failed to connect to rendezvous point at {}: {}",
                    address,
                    error
                );
                return;
            }
            SwarmEvent::Behaviour(MyEvent::Rendezvous(rendezvous::Event::Discovered {
                registrations,
                ..
            })) => {
                for ((_, peer), registration) in registrations {
                    for address in registration.record.addresses() {
                        log::info!("Discovered peer {} at {}", peer, address);

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
            })) if peer != rendezvous_point => {
                log::info!("Ping to {} is {}ms", peer, rtt.as_millis())
            }
            other => {
                log::debug!("Unhandled {:?}", other);
            }
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
