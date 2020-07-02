mod cli;
pub mod transport;

pub use cli::Opt;

use std::{
    io,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
    collections::HashMap,
};

use anyhow::{Result};
use futures::{future, prelude::*};
use libp2p::{
    core::{
        either::EitherError,
        muxing::StreamMuxerBox,
        transport::{boxed::Boxed, timeout::TransportTimeoutError},
        upgrade::{SelectUpgrade, Version},
        UpgradeError,
    },
    dns::{DnsConfig, DnsErr},
    identity,
    mplex::MplexConfig,
    ping::{Ping, PingConfig},
    secio::{SecioConfig, SecioError},
    swarm::SwarmBuilder,
    yamux, Multiaddr, PeerId, Swarm, Transport,
};
use tokio::net::TcpStream;
use tokio_socks::{tcp::Socks5Stream, IntoTargetAddr};

use crate::transport::TorTokioTcpConfig;

/// Entry point to run the ping-pong application as a dialer.
pub async fn run_dialer(addr: Multiaddr) -> Result<()> {
    let map = HashMap::new();
    let config = PingConfig::new()
        .with_keep_alive(true)
        .with_interval(Duration::from_secs(1));
    let mut swarm = crate::build_swarm(config, map)?;

    Swarm::dial_addr(&mut swarm, addr).unwrap();

    future::poll_fn(move |cx: &mut Context| loop {
        match swarm.poll_next_unpin(cx) {
            Poll::Ready(Some(event)) => println!("{:?}", event),
            Poll::Ready(None) => return Poll::Ready(()),
            Poll::Pending => return Poll::Pending,
        }
    })
    .await;

    Ok(())
}

/// Entry point to run the ping-pong application as a listener.
pub async fn run_listener(onion: Multiaddr) -> Result<()> {
    let map = onion_port_map(onion.clone());
    println!("Onion service: {}", onion);

    let config = PingConfig::new().with_keep_alive(true);
    let mut swarm = crate::build_swarm(config, map)?;

    Swarm::listen_on(&mut swarm, onion.clone())?;

    future::poll_fn(move |cx: &mut Context| loop {
        match swarm.poll_next_unpin(cx) {
            Poll::Ready(Some(event)) => println!("{:?}", event),
            Poll::Ready(None) => return Poll::Ready(()),
            Poll::Pending => return Poll::Pending,
        }
    })
    .await;

    Ok(())
}

/// Build a libp2p swarm (also called a switch).
pub fn build_swarm(config: PingConfig, map: HashMap<Multiaddr, u16>) -> Result<Swarm<Ping>> {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());

    let transport = crate::build_transport(id_keys, map)?;
    let behaviour = Ping::new(config);

    let swarm = SwarmBuilder::new(transport, behaviour, peer_id)
        .executor(Box::new(TokioExecutor))
        .build();

    Ok(swarm)
}

fn onion_port_map(onion: Multiaddr) -> HashMap<Multiaddr, u16> {
    let mut map = HashMap::new();
    // FIMXE: This shouldn't be hard coded.
    map.insert(onion, 7777);
    map
}

struct TokioExecutor;

impl libp2p::core::Executor for TokioExecutor {
    fn exec(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        tokio::spawn(future);
    }
}

/// Builds a libp2p transport with the following features:
/// - TCp connectivity
/// - DNS name resolution
/// - Authentication via secio
/// - Multiplexing via yamux or mplex
pub fn build_transport(keypair: identity::Keypair, map: HashMap<Multiaddr, u16>) -> anyhow::Result<PingPongTransport> {
    let transport = TorTokioTcpConfig::new().nodelay(true).onion_map(map);
    let transport = DnsConfig::new(transport)?;

    let transport = transport
        .upgrade(Version::V1)
        .authenticate(SecioConfig::new(keypair))
        .multiplex(SelectUpgrade::new(
            yamux::Config::default(),
            MplexConfig::new(),
        ))
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .timeout(Duration::from_secs(20))
        .boxed();

    Ok(transport)
}

/// libp2p `Transport` for the ping-pong application.
pub type PingPongTransport = Boxed<
    (PeerId, StreamMuxerBox),
    TransportTimeoutError<
        EitherError<
            EitherError<DnsErr<io::Error>, UpgradeError<SecioError>>,
            UpgradeError<EitherError<io::Error, io::Error>>,
        >,
    >,
>;

/// Connect to the Tor socks5 proxy socket.
pub async fn connect_tor_socks_proxy<'a>(dest: impl IntoTargetAddr<'a>, port: u16) -> Result<TcpStream> {
    let tor_sock = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    let stream = Socks5Stream::connect(tor_sock, dest).await?;
    Ok(stream.into_inner())
}
