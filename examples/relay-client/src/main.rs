use std::{error::Error, num::NonZeroU8};

use clap::Parser;
use futures::stream::StreamExt;
use libp2p::{
    core::multiaddr::Multiaddr,
    identify, identity, noise, ping,
    relay::{self, autorelay},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "libp2p relay client")]
struct Opts {
    /// Fixed value used to derive a deterministic peer id.
    #[arg(long)]
    secret_key_seed: u8,

    /// List of relay addresses
    #[arg(long = "relay", required = true)]
    relays: Vec<Multiaddr>,

    /// Maximum number of relay reservations autorelay should maintain.
    #[arg(long, default_value_t = 2)]
    max_reservations: u8,
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay_client: relay::client::Behaviour,
    autorelay: autorelay::Behaviour,
    identify: identify::Behaviour,
    ping: ping::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let opts = Opts::parse();

    let max_reservations = NonZeroU8::new(opts.max_reservations)
        .ok_or("--max-reservations must be greater than zero")?;
    let autorelay_config = autorelay::Config::default().set_max_reservations(max_reservations.get());

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(generate_ed25519(opts.secret_key_seed))
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_dns()?
        .with_relay_client(noise::Config::new, yamux::Config::default)?
        .with_behaviour(|keypair, relay_behaviour| Behaviour {
            relay_client: relay_behaviour,
            autorelay: autorelay::Behaviour::new_with_config(autorelay_config),
            identify: identify::Behaviour::new(identify::Config::new(
                "/autorelay-example/0.1.0".to_owned(),
                keypair.public(),
            )),
            ping: ping::Behaviour::new(ping::Config::new()),
        })?
        .build();

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?)?;

    let local_peer_id = *swarm.local_peer_id();
    tracing::info!(%local_peer_id, "Local peer id");

    for addr in &opts.relays {
        tracing::info!(%addr, "Dialing relay");
        swarm.dial(addr.clone())?;
    }
    
    loop {
        tokio::select! {
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { address, .. } => {
                    tracing::info!(%address, "Listening");
                }
                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } => {
                    tracing::info!(peer=%peer_id, address=%endpoint.get_remote_address(), "Connected");
                }
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    tracing::info!(peer=%peer_id, "Disconnected");
                }
                SwarmEvent::ExternalAddrConfirmed { address } => {
                    tracing::info!(%address, "External address confirmed");
                }
                SwarmEvent::ExternalAddrExpired { address } => {
                    tracing::info!(%address, "External address expired");
                }
                SwarmEvent::Behaviour(BehaviourEvent::RelayClient(
                    relay::client::Event::ReservationReqAccepted {
                        relay_peer_id, renewal, ..
                    },
                )) => {
                    if renewal {
                        tracing::info!(%relay_peer_id, "Reservation renewed");
                    } else {
                        tracing::info!(%relay_peer_id, "Reservation accepted");
                    }
                }
                SwarmEvent::Behaviour(BehaviourEvent::RelayClient(event)) => {
                    tracing::debug!(?event, "Relay client event");
                }
                SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
                    peer_id, info, ..
                })) => {
                    tracing::debug!(peer=%peer_id, protocols=?info.protocols, "Identify received");
                }
                SwarmEvent::Behaviour(BehaviourEvent::Identify(_)) => {}
                SwarmEvent::Behaviour(BehaviourEvent::Ping(_)) => {}
                SwarmEvent::Behaviour(BehaviourEvent::Autorelay(_)) => {}
                _ => {}
            },
        }
    }
}

fn generate_ed25519(secret_key_seed: u8) -> identity::Keypair {
    let mut bytes = [0u8; 32];
    bytes[0] = secret_key_seed;
    identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
}
