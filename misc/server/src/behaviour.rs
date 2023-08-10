use libp2p::autonat;
use libp2p::identify;
use libp2p::kad::{record::store::MemoryStore, Kademlia, KademliaConfig, KademliaEvent};
use libp2p::ping;
use libp2p::relay;
use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::{identity, swarm::NetworkBehaviour, Multiaddr, PeerId};
use std::str::FromStr;
use std::time::Duration;

const BOOTNODES: [&str; 4] = [
    "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
    "QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
    "QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
    "QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
];

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "Event", event_process = false)]
pub struct Behaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    pub kademlia: Toggle<Kademlia<MemoryStore>>,
    autonat: Toggle<autonat::Behaviour>,
}

impl Behaviour {
    pub fn new(pub_key: identity::PublicKey, enable_kademlia: bool, enable_autonat: bool) -> Self {
        let kademlia = if enable_kademlia {
            let mut kademlia_config = KademliaConfig::default();
            // Instantly remove records and provider records.
            //
            // TODO: Replace hack with option to disable both.
            kademlia_config.set_record_ttl(Some(Duration::from_secs(0)));
            kademlia_config.set_provider_record_ttl(Some(Duration::from_secs(0)));
            let mut kademlia = Kademlia::with_config(
                pub_key.to_peer_id(),
                MemoryStore::new(pub_key.to_peer_id()),
                kademlia_config,
            );
            let bootaddr = Multiaddr::from_str("/dnsaddr/bootstrap.libp2p.io").unwrap();
            for peer in &BOOTNODES {
                kademlia.add_address(&PeerId::from_str(peer).unwrap(), bootaddr.clone());
            }
            kademlia.bootstrap().unwrap();
            Some(kademlia)
        } else {
            None
        }
        .into();

        let autonat = if enable_autonat {
            Some(autonat::Behaviour::new(
                PeerId::from(pub_key.clone()),
                Default::default(),
            ))
        } else {
            None
        }
        .into();

        Self {
            relay: relay::Behaviour::new(PeerId::from(pub_key.clone()), Default::default()),
            ping: ping::Behaviour::new(ping::Config::new()),
            identify: identify::Behaviour::new(
                identify::Config::new("ipfs/0.1.0".to_string(), pub_key).with_agent_version(
                    format!("rust-libp2p-server/{}", env!("CARGO_PKG_VERSION")),
                ),
            ),
            kademlia,
            autonat,
        }
    }
}

#[derive(Debug)]
pub enum Event {
    Ping(ping::Event),
    Identify(Box<identify::Event>),
    Relay(relay::Event),
    Kademlia(KademliaEvent),
    Autonat(autonat::Event),
}

impl From<ping::Event> for Event {
    fn from(event: ping::Event) -> Self {
        Event::Ping(event)
    }
}

impl From<identify::Event> for Event {
    fn from(event: identify::Event) -> Self {
        Event::Identify(Box::new(event))
    }
}

impl From<relay::Event> for Event {
    fn from(event: relay::Event) -> Self {
        Event::Relay(event)
    }
}

impl From<KademliaEvent> for Event {
    fn from(event: KademliaEvent) -> Self {
        Event::Kademlia(event)
    }
}

impl From<autonat::Event> for Event {
    fn from(event: autonat::Event) -> Self {
        Event::Autonat(event)
    }
}
