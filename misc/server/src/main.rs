use base64::Engine;
use futures::executor::block_on;
use futures::future::Either;
use futures::stream::StreamExt;
use futures_timer::Delay;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::upgrade;
use libp2p::dns;
use libp2p::identify;
use libp2p::identity;
use libp2p::identity::PeerId;
use libp2p::kad;
use libp2p::metrics::{Metrics, Recorder};
use libp2p::multiaddr::Protocol;
use libp2p::noise;
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::tcp;
use libp2p::yamux;
use libp2p::Multiaddr;
use libp2p::Transport;
use log::{debug, info};
use prometheus_client::metrics::info::Info;
use prometheus_client::registry::Registry;
use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::str::FromStr;
use std::task::Poll;
use std::thread;
use std::time::Duration;
use structopt::StructOpt;
use zeroize::Zeroizing;

mod behaviour;
mod config;
mod metric_server;

const BOOTSTRAP_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, StructOpt)]
#[structopt(name = "libp2p server", about = "A rust-libp2p server binary.")]
struct Opt {
    /// Path to IPFS config file.
    #[structopt(long)]
    config: PathBuf,

    /// Metric endpoint path.
    #[structopt(long, default_value = "/metrics")]
    metrics_path: String,

    /// Whether to run the libp2p Kademlia protocol and join the IPFS DHT.
    #[structopt(long)]
    enable_kademlia: bool,

    /// Whether to run the libp2p Autonat protocol.
    #[structopt(long)]
    enable_autonat: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let opt = Opt::from_args();

    let config = Zeroizing::new(config::Config::from_file(opt.config.as_path())?);

    let (local_peer_id, local_keypair) = {
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

        (peer_id, keypair)
    };
    println!("Local peer id: {local_peer_id:?}");

    let transport = {
        let tcp_transport =
            tcp::async_io::Transport::new(tcp::Config::new().port_reuse(true).nodelay(true))
                .upgrade(upgrade::Version::V1)
                .authenticate(noise::Config::new(&local_keypair)?)
                .multiplex(yamux::Config::default())
                .timeout(Duration::from_secs(20));

        let quic_transport = {
            let mut config = libp2p_quic::Config::new(&local_keypair);
            config.support_draft_29 = true;
            libp2p_quic::async_std::Transport::new(config)
        };

        block_on(dns::DnsConfig::system(
            libp2p::core::transport::OrTransport::new(quic_transport, tcp_transport),
        ))
        .unwrap()
        .map(|either_output, _| match either_output {
            Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
        })
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
        .boxed()
    };

    let behaviour = behaviour::Behaviour::new(
        local_keypair.public(),
        opt.enable_kademlia,
        opt.enable_autonat,
    );
    let mut swarm =
        SwarmBuilder::with_async_std_executor(transport, behaviour, local_peer_id).build();

    if config.addresses.swarm.is_empty() {
        log::warn!("No listen addresses configured.");
    }
    for address in config.addresses.swarm.iter().filter(filter_out_ipv6_quic) {
        match swarm.listen_on(address.clone()) {
            Ok(_) => {}
            Err(e @ libp2p::TransportError::MultiaddrNotSupported(_)) => {
                log::warn!("Failed to listen on {address}, continuing anyways, {e}")
            }
            Err(e) => return Err(e.into()),
        }
    }
    if config.addresses.append_announce.is_empty() {
        log::warn!("No external addresses configured.");
    }
    for address in config
        .addresses
        .append_announce
        .iter()
        .filter(filter_out_ipv6_quic)
    {
        swarm.add_external_address(address.clone())
    }
    log::info!(
        "External addresses: {:?}",
        swarm.external_addresses().collect::<Vec<_>>()
    );

    let mut metric_registry = Registry::default();
    let metrics = Metrics::new(&mut metric_registry);
    let build_info = Info::new(vec![("version".to_string(), env!("CARGO_PKG_VERSION"))]);
    metric_registry.register(
        "build",
        "A metric with a constant '1' value labeled by version",
        build_info,
    );
    thread::spawn(move || block_on(metric_server::run(metric_registry, opt.metrics_path)));

    let mut bootstrap_timer = Delay::new(BOOTSTRAP_INTERVAL);

    block_on(async {
        loop {
            if let Poll::Ready(()) = futures::poll!(&mut bootstrap_timer) {
                bootstrap_timer.reset(BOOTSTRAP_INTERVAL);
                let _ = swarm
                    .behaviour_mut()
                    .kademlia
                    .as_mut()
                    .map(|k| k.bootstrap());
            }

            match swarm.next().await.expect("Swarm not to terminate.") {
                SwarmEvent::Behaviour(behaviour::Event::Identify(e)) => {
                    info!("{:?}", e);
                    metrics.record(&*e);

                    if let identify::Event::Received {
                        peer_id,
                        info:
                            identify::Info {
                                listen_addrs,
                                protocols,
                                ..
                            },
                    } = *e
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
                SwarmEvent::Behaviour(behaviour::Event::Ping(e)) => {
                    debug!("{:?}", e);
                    metrics.record(&e);
                }
                SwarmEvent::Behaviour(behaviour::Event::Kademlia(e)) => {
                    debug!("{:?}", e);
                    metrics.record(&e);
                }
                SwarmEvent::Behaviour(behaviour::Event::Relay(e)) => {
                    info!("{:?}", e);
                    metrics.record(&e)
                }
                SwarmEvent::Behaviour(behaviour::Event::Autonat(e)) => {
                    info!("{:?}", e);
                    // TODO: Add metric recording for `NatStatus`.
                    // metrics.record(&e)
                }
                e => {
                    if let SwarmEvent::NewListenAddr { address, .. } = &e {
                        println!("Listening on {address:?}");
                    }

                    metrics.record(&e)
                }
            }
        }
    })
}

fn filter_out_ipv6_quic(m: &&Multiaddr) -> bool {
    let mut iter = m.iter();

    let is_ipv6_quic = matches!(
        (iter.next(), iter.next(), iter.next()),
        (
            Some(Protocol::Ip6(_)),
            Some(Protocol::Udp(_)),
            Some(Protocol::Quic | Protocol::QuicV1)
        )
    );

    if is_ipv6_quic {
        log::warn!("Ignoring IPv6 QUIC address {m}. Currently unsupported. See https://github.com/libp2p/rust-libp2p/issues/4165.");
        return false;
    }

    true
}
