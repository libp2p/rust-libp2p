use std::{str::FromStr, time::Duration};

use libp2p::{
    autonat, identify, identity, kad, ping, relay,
    swarm::{behaviour::toggle::Toggle, NetworkBehaviour, StreamProtocol},
    Multiaddr, PeerId,
};

const BOOTNODES: [&str; 4] = [
    "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
    "QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
    "QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
    "QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
];

const IPFS_PROTO_NAME: StreamProtocol = StreamProtocol::new("/ipfs/kad/1.0.0");

#[derive(NetworkBehaviour)]
pub(crate) struct Behaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    pub(crate) kademlia: Toggle<kad::Behaviour<kad::store::MemoryStore>>,
    autonat: Toggle<autonat::Behaviour>,
}

impl Behaviour {
    pub(crate) fn new(
        pub_key: identity::PublicKey,
        enable_kademlia: bool,
        enable_autonat: bool,
    ) -> Self {
        let kademlia = if enable_kademlia {
            let mut kademlia_config = kad::Config::new(IPFS_PROTO_NAME);
            // Instantly remove records and provider records.
            //
            // TODO: Replace hack with option to disable both.
            kademlia_config.set_record_ttl(Some(Duration::from_secs(0)));
            kademlia_config.set_provider_record_ttl(Some(Duration::from_secs(0)));
            let mut kademlia = kad::Behaviour::with_config(
                pub_key.to_peer_id(),
                kad::store::MemoryStore::new(pub_key.to_peer_id()),
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
