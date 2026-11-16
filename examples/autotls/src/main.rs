use std::{error::Error, net::Ipv4Addr};

use futures::StreamExt;
use libp2p::{
    core::{Transport, multiaddr::Protocol, transport::upgrade::Version},
    identify,
    multiaddr::Multiaddr,
    noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, tls, websocket, yamux,
};
use libp2p_autotls::{
    self as autotls, broker::DEFAULT_FORGE_DOMAIN, encoding, storage::MemCertStore,
};
use tracing_subscriber::EnvFilter;

/// The plain TCP port the broker dials to verify reachability.
const TCP_PORT: u16 = 4001;
/// The Secure WebSocket port browsers connect to.
const WSS_PORT: u16 = 4002;

#[derive(NetworkBehaviour)]
struct Behaviour {
    autotls: autotls::Behaviour<MemCertStore>,
    identify: identify::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,libp2p_autotls=debug")),
        )
        .try_init()
        .ok();

    let keypair = libp2p::identity::Keypair::generate_ed25519();
    let peer_id = keypair.public().to_peer_id();
    println!("Local peer id: {peer_id}");
    println!(
        "Requesting a certificate for *.{}.{DEFAULT_FORGE_DOMAIN}",
        encoding::peer_id_label(peer_id)
    );

    let autotls = autotls::Behaviour::new(
        &keypair,
        autotls::acme::AcmeConfig::production(),
        MemCertStore::new(),
    )
    .await;
    let resolver = autotls.certificate_resolver();

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            (noise::Config::new, tls::Config::new),
            yamux::Config::default,
        )?
        .with_other_transport(move |key| {
            let mut ws = websocket::Config::new(tcp::tokio::Transport::new(tcp::Config::default()));
            ws.set_tls_config(websocket::tls::Config::new_with_server_cert_resolver(
                resolver,
            ));
            Ok::<_, Box<dyn Error + Send + Sync>>(
                ws.upgrade(Version::V1Lazy)
                    .authenticate(noise::Config::new(key)?)
                    .multiplex(yamux::Config::default()),
            )
        })?
        .with_behaviour(|key| Behaviour {
            autotls,
            identify: identify::Behaviour::new(identify::Config::new(
                "/ipfs/id/1.0.0".into(),
                key.public(),
            )),
        })?
        .build();

    swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{TCP_PORT}").parse()?)?;
    swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{WSS_PORT}/tls/ws").parse()?)?;
    println!("Waiting for a public address; AutoTLS will request a certificate once one is found.");

    let mut has_public_address = false;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("Listening on {address}");
                if public_tcp_addr(&address) && !has_public_address {
                    println!(
                        "Found public address {address}; obtaining a certificate from Let's \
                         Encrypt via the broker (this can take a couple of minutes)..."
                    );
                    swarm.add_external_address(address);
                    has_public_address = true;
                }
            }
            SwarmEvent::ExternalAddrConfirmed { address } => {
                println!("External address confirmed: {address}")
            }
            SwarmEvent::Behaviour(BehaviourEvent::Autotls(event)) => match event {
                autotls::Event::CertificateObtained { not_after_unix } => {
                    println!(
                        "SUCCESS: obtained certificate, expires at unix timestamp {not_after_unix}"
                    )
                }
                autotls::Event::IssuanceFailed(error) => {
                    println!("FAILED: certificate issuance failed: {error}")
                }
            },
            _ => {}
        }
    }
}

fn public_tcp_addr(addr: &Multiaddr) -> bool {
    let mut protocols = addr.iter();
    matches!(protocols.next(), Some(Protocol::Ip4(ip)) if is_global_ipv4(ip))
        && matches!(protocols.next(), Some(Protocol::Tcp(_)))
        && protocols.next().is_none()
}

fn is_global_ipv4(ip: Ipv4Addr) -> bool {
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast())
}
