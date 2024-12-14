use std::{error::Error, path::PathBuf, str::FromStr};

use base64::Engine;
use clap::Parser;
use futures::stream::StreamExt;
use libp2p::{
    identify, identity,
    identity::PeerId,
    kad,
    metrics::{Metrics, Recorder},
    noise,
    swarm::SwarmEvent,
    tcp, yamux,
};
use prometheus_client::{metrics::info::Info, registry::Registry};
use tracing_subscriber::EnvFilter;
use zeroize::Zeroizing;

mod behaviour;
mod config;
mod http_service;

#[derive(Debug, Parser)]
#[clap(name = "libp2p server", about = "A rust-libp2p server binary.")]
struct Opts {
    /// Path to IPFS config file.
    #[clap(long)]
    config: PathBuf,

    /// Metric endpoint path.
    #[clap(long, default_value = "/metrics")]
    metrics_path: String,

    /// Whether to run the libp2p Kademlia protocol and join the IPFS DHT.
    #[clap(long)]
    enable_kademlia: bool,

    /// Whether to run the libp2p Autonat protocol.
    #[clap(long)]
    enable_autonat: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let opt = Opts::parse();

    let config = Zeroizing::new(config::Config::from_file(opt.config.as_path())?);

    let mut metric_registry = Registry::default();

    let local_keypair = {
        let keypair = identity::Keypair::from_protobuf_encoding(&Zeroizing::new(
            base64::engine::general_purpose::STANDARD
                .decode(config.identity.priv_key.as_bytes())?,
        ))?;

        let peer_id = keypair.public().into();
        assert_eq!(
            PeerId::from_str(&config.identity.peer_id)?,
            peer_id,
            "Expect peer id derived from private key and peer id retrieved from config to match."
        );

        keypair
    };

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_dns()?
        .with_websocket(noise::Config::new, yamux::Config::default)
        .await?
        .with_bandwidth_metrics(&mut metric_registry)
        .with_behaviour(|key| {
            behaviour::Behaviour::new(key.public(), opt.enable_kademlia, opt.enable_autonat)
        })?
        .build();

    if config.addresses.swarm.is_empty() {
        tracing::warn!("No listen addresses configured");
    }
    for address in &config.addresses.swarm {
        match swarm.listen_on(address.clone()) {
            Ok(_) => {}
            Err(e @ libp2p::TransportError::MultiaddrNotSupported(_)) => {
                tracing::warn!(%address, "Failed to listen on address, continuing anyways, {e}")
            }
            Err(e) => return Err(e.into()),
        }
    }

    if config.addresses.append_announce.is_empty() {
        tracing::warn!("No external addresses configured");
    }
    for address in &config.addresses.append_announce {
        swarm.add_external_address(address.clone())
    }
    tracing::info!(
        "External addresses: {:?}",
        swarm.external_addresses().collect::<Vec<_>>()
    );

    let metrics = Metrics::new(&mut metric_registry);
    let build_info = Info::new(vec![("version".to_string(), env!("CARGO_PKG_VERSION"))]);
    metric_registry.register(
        "build",
        "A metric with a constant '1' value labeled by version",
        build_info,
    );
    tokio::spawn(async move {
        if let Err(e) = http_service::metrics_server(metric_registry, opt.metrics_path).await {
            tracing::error!("Metrics server failed: {e}");
        }
    });

    loop {
        let event = swarm.next().await.expect("Swarm not to terminate.");
        metrics.record(&event);
        match event {
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Identify(e)) => {
                tracing::info!("{:?}", e);
                metrics.record(&e);

                if let identify::Event::Received {
                    peer_id,
                    info:
                        identify::Info {
                            listen_addrs,
                            protocols,
                            ..
                        },
                    ..
                } = e
                {
                    if protocols.iter().any(|p| *p == kad::PROTOCOL_NAME) {
                        for addr in listen_addrs {
                            swarm
                                .behaviour_mut()
                                .kademlia
                                .as_mut()
                                .map(|k| k.add_address(&peer_id, addr));
                        }
                    }
                }
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Ping(e)) => {
                tracing::debug!("{:?}", e);
                metrics.record(&e);
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Kademlia(e)) => {
                tracing::debug!("{:?}", e);
                metrics.record(&e);
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Relay(e)) => {
                tracing::info!("{:?}", e);
                metrics.record(&e)
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Autonat(e)) => {
                tracing::info!("{:?}", e);
                // TODO: Add metric recording for `NatStatus`.
                // metrics.record(&e)
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                tracing::info!(%address, "Listening on address");
            }
            _ => {}
        }
    }
}
