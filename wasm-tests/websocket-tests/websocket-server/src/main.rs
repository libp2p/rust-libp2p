use std::env;
use anyhow::Context;
use futures::stream::StreamExt;
use libp2p::identity::{ed25519, Keypair};
use libp2p::swarm::SwarmEvent;
use libp2p::{noise, yamux, Multiaddr, tcp, websocket};
use std::error::Error;
use std::time::Duration;
use sha2::{Digest as ShaDigestTrait, Sha256};
use tracing_subscriber::EnvFilter;
use libp2p_core::Transport;

mod behaviour;

const BOOTSTRAP_INTERVAL: Duration = Duration::from_secs(5 * 60);
const KEYPAIR_SEED_PHRASE: &str = "libp2p-websocket-server"; // 12D3KooWNbGStz2dMnBZYdAvuchWW4xfeHrjHboP1hiq6XexRKEf

fn generate_keypair(seed: &str) -> Result<Keypair, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    hasher.update(seed);
    let secret_key = ed25519::SecretKey::try_from_bytes(&mut hasher.finalize())?;
    let keypair = ed25519::Keypair::from(secret_key);
    Ok(Keypair::from(keypair))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let server_listen_multiaddr = env::var("SERVER_LISTEN_MULTIADDRESS")
        .map_err(|_| "SERVER_LISTEN_MULTIADDRESS environment variable not found")?;

    let local_keypair = generate_keypair(KEYPAIR_SEED_PHRASE)?;
    tracing::info!("Local peer id: {:?}", local_keypair.public());

    let tcp_config = tcp::Config::new().nodelay(false).port_reuse(true);

    let wss_transport = {
        let ws_trans = websocket::WsConfig::new(tcp::tokio::Transport::new(tcp_config.clone()))
            .or_transport(tcp::tokio::Transport::new(tcp_config));

        libp2p::dns::tokio::Transport::system(ws_trans)?.boxed()
    };

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_keypair)
        .with_tokio()
        .with_other_transport(|local_key| {
            Ok(wss_transport
                .upgrade(libp2p_core::upgrade::Version::V1)
                .authenticate(
                    noise::Config::new(&local_key)
                        .context("failed to initialise noise")?,
                )
                .multiplex(yamux::Config::default()))
        })?
        .with_behaviour(|key| {
            behaviour::Behaviour::new(key.public())
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();


    match swarm.listen_on(server_listen_multiaddr.parse::<Multiaddr>()?.clone()) {
        Ok(_) => {}
        Err(e @ libp2p::TransportError::MultiaddrNotSupported(_)) => {
            tracing::warn!(server_listen_multiaddr, "Failed to listen on address, continuing anyways, {e}")
        }
        Err(e) => return Err(e.into()),
    }

    swarm.add_external_address(server_listen_multiaddr.parse::<Multiaddr>()?.clone());

    tracing::info!(
        "External addresses: {:?}",
        swarm.external_addresses().collect::<Vec<_>>()
    );

    loop {

        let event = swarm.next().await.expect("Swarm not to terminate.");
        tracing::debug!("Event: {:?}", event);

        match event {
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Identify(e)) => {
                tracing::info!("{:?}", e);
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                tracing::info!(%address, "Listening on address");
            }
            _ => {
                tracing::debug!("Unhandled swarm event: {:?}", event);
            }
        }
    }
}
