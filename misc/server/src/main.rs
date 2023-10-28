use base64::Engine;
use clap::Parser;
use futures::stream::StreamExt;
use futures_timer::Delay;
use libp2p::identity;
use libp2p::identity::PeerId;
use libp2p::kad;
use libp2p::metrics::{Metrics, Recorder};
use libp2p::swarm::SwarmEvent;
use libp2p::tcp;
use libp2p::{identify, noise, yamux};
use log::{debug, info, warn};
use prometheus_client::metrics::info::Info;
use prometheus_client::registry::Registry;
use std::error::Error;
use std::path::PathBuf;
use std::str::FromStr;
use std::task::Poll;
use std::time::Duration;
use zeroize::Zeroizing;

mod behaviour;
mod config;
mod http_service;

const BOOTSTRAP_INTERVAL: Duration = Duration::from_secs(5 * 60);

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
    env_logger::init();

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
            tcp::Config::default().port_reuse(true).nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_dns()?
        .with_bandwidth_metrics(&mut metric_registry)
        .with_behaviour(|key| {
            behaviour::Behaviour::new(key.public(), opt.enable_kademlia, opt.enable_autonat)
        })?
        .build();

    if config.addresses.swarm.is_empty() {
        warn!("No listen addresses configured.");
    }
    for address in &config.addresses.swarm {
        match swarm.listen_on(address.clone()) {
            Ok(_) => {}
            Err(e @ libp2p::TransportError::MultiaddrNotSupported(_)) => {
                warn!("Failed to listen on {address}, continuing anyways, {e}")
            }
            Err(e) => return Err(e.into()),
        }
    }

    if config.addresses.append_announce.is_empty() {
        warn!("No external addresses configured.");
    }
    for address in &config.addresses.append_announce {
        swarm.add_external_address(address.clone())
    }
    info!(
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
            log::error!("Metrics server failed: {e}");
        }
    });

    let mut bootstrap_timer = Delay::new(BOOTSTRAP_INTERVAL);

    loop {
        if let Poll::Ready(()) = futures::poll!(&mut bootstrap_timer) {
            bootstrap_timer.reset(BOOTSTRAP_INTERVAL);
            let _ = swarm
                .behaviour_mut()
                .kademlia
                .as_mut()
                .map(|k| k.bootstrap());
        }

        let event = swarm.next().await.expect("Swarm not to terminate.");
        metrics.record(&event);
        match event {
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Identify(e)) => {
                info!("{:?}", e);
                metrics.record(&e);

                if let identify::Event::Received {
                    peer_id,
                    info:
                        identify::Info {
                            listen_addrs,
                            protocols,
                            ..
                        },
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
                debug!("{:?}", e);
                metrics.record(&e);
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Kademlia(e)) => {
                debug!("{:?}", e);
                metrics.record(&e);
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Relay(e)) => {
                info!("{:?}", e);
                metrics.record(&e)
            }
            SwarmEvent::Behaviour(behaviour::BehaviourEvent::Autonat(e)) => {
                info!("{:?}", e);
                // TODO: Add metric recording for `NatStatus`.
                // metrics.record(&e)
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {address:?}");
            }
            _ => {}
        }
    }
}
