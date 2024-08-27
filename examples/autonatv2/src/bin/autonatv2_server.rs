use std::{error::Error, net::Ipv4Addr, time::Duration};

use cfg_if::cfg_if;
use clap::Parser;
use libp2p::{
    autonat,
    futures::StreamExt,
    identify, identity,
    multiaddr::Protocol,
    noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, SwarmBuilder,
};
use rand::rngs::OsRng;

#[derive(Debug, Parser)]
#[clap(name = "libp2p autonatv2 server")]
struct Opt {
    #[clap(short, long, default_value_t = 0)]
    listen_port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    cfg_if! {
        if #[cfg(feature = "jaeger")] {
            use tracing_subscriber::layer::SubscriberExt;
            use opentelemetry_sdk::runtime::Tokio;
            let tracer = opentelemetry_jaeger::new_agent_pipeline()
                .with_endpoint("jaeger:6831")
                .with_service_name("autonatv2")
                .install_batch(Tokio)?;
            let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
            let subscriber = tracing_subscriber::Registry::default()
                .with(telemetry);
        } else {
            let subscriber = tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .finish();
        }
    }
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let opt = Opt::parse();

    let mut swarm = SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_dns()?
        .with_behaviour(|key| Behaviour::new(key.public()))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    swarm.listen_on(
        Multiaddr::empty()
            .with(Protocol::Ip4(Ipv4Addr::UNSPECIFIED))
            .with(Protocol::Tcp(opt.listen_port)),
    )?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => println!("Listening on {address:?}"),
            SwarmEvent::Behaviour(event) => println!("{event:?}"),
            e => println!("{e:?}"),
        }
    }
}

#[derive(NetworkBehaviour)]
pub struct Behaviour {
    autonat: autonat::v2::server::Behaviour,
    identify: identify::Behaviour,
}

impl Behaviour {
    pub fn new(key: identity::PublicKey) -> Self {
        Self {
            autonat: autonat::v2::server::Behaviour::new(OsRng),
            identify: identify::Behaviour::new(identify::Config::new("/ipfs/0.1.0".into(), key)),
        }
    }
}
