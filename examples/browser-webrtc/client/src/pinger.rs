use super::fetch_server_addr;
use async_channel::Sender;
use futures::StreamExt;
use libp2p::core::Multiaddr;
use libp2p::identity;
use libp2p::identity::PeerId;
use libp2p::ping;
use libp2p::swarm::{keep_alive, NetworkBehaviour, SwarmBuilder, SwarmEvent};
use libp2p::{multiaddr, swarm};
use std::convert::From;

pub async fn start_pinger(sendr: Sender<f32>) -> Result<(), PingerError> {
    let addr_fut = fetch_server_addr();

    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    let mut swarm = SwarmBuilder::with_wasm_executor(
        libp2p_webrtc_websys::Transport::new(libp2p_webrtc_websys::Config::new(&local_key)).boxed(),
        Behaviour {
            ping: ping::Behaviour::new(ping::Config::new()),
            keep_alive: keep_alive::Behaviour,
        },
        local_peer_id,
    )
    .build();

    log::info!("Running pinger with peer_id: {}", swarm.local_peer_id());

    let addr = addr_fut.await;

    log::info!("Dialing {}", addr);

    swarm.dial(addr.parse::<Multiaddr>()?)?;

    loop {
        match swarm.next().await.unwrap() {
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event { result: Err(e), .. })) => {
                log::error!("Ping failed: {:?}", e);
                let _result = sendr.send(-1.).await;
            }
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                peer,
                result: Ok(rtt),
                ..
            })) => {
                log::info!("Ping successful: {rtt:?}, {peer}");
                let _result = sendr.send(rtt.as_micros() as f32 / 1000.).await;
                log::debug!("RTT Sent");
            }
            evt => log::info!("Swarm event: {:?}", evt),
        }
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
}

#[derive(Debug)]
pub enum PingerError {
    AddrParse(std::net::AddrParseError),
    MultiaddrParse(multiaddr::Error),
    Dial(swarm::DialError),
}

impl From<libp2p::multiaddr::Error> for PingerError {
    fn from(err: libp2p::multiaddr::Error) -> Self {
        PingerError::MultiaddrParse(err)
    }
}

impl From<libp2p::swarm::DialError> for PingerError {
    fn from(err: libp2p::swarm::DialError) -> Self {
        PingerError::Dial(err)
    }
}
