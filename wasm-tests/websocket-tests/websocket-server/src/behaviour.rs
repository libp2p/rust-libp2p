use libp2p::identify;
use libp2p::{identity, swarm::NetworkBehaviour, Multiaddr, PeerId};

#[derive(NetworkBehaviour)]
pub(crate) struct Behaviour {
    identify: identify::Behaviour
}

impl Behaviour {
    pub(crate) fn new(
        pub_key: identity::PublicKey,
    ) -> Self {

        Self {
            identify: identify::Behaviour::new(
                identify::Config::new("/ipfs/id/1.0.0".to_string(), pub_key).with_agent_version(
                    format!("rust-libp2p-server/{}", env!("CARGO_PKG_VERSION")),
                ),
            )
        }
    }
}
