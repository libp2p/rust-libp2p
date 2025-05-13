// Copyright 2020 Parity Technologies (UK) Ltd.
// Copyright 2021 Protocol Labs.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

#![doc = include_str!("../README.md")]

use std::{
    error::Error,
    net::{Ipv4Addr, Ipv6Addr},
};

use clap::Parser;
use futures::StreamExt;
use libp2p::{
    core::{multiaddr::Protocol, muxing::StreamMuxerBox, Multiaddr, Transport},
    identify, identity, noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, PeerId, Swarm,
};
use libp2p_webrtc as webrtc;
use rand::thread_rng;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let opt = Opt::parse();

    // Create a static known PeerId based on given secret
    let local_key: identity::Keypair = generate_ed25519(opt.secret_key_seed);
    let local_peer_id = PeerId::from(local_key.public());
    info!(?local_peer_id, "Local peer id");

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_other_transport(|id_keys| {
            Ok(webrtc::tokio::Transport::new(
                id_keys.clone(),
                webrtc::tokio::Certificate::generate(&mut thread_rng())?,
            )
            .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn))))
        })?
        .with_dns()?
        .with_websocket(noise::Config::new, yamux::Config::default)
        .await?
        .with_behaviour(|key| Behaviour {
            relay: relay::Behaviour::new(key.public().to_peer_id(), Default::default()),
            ping: ping::Behaviour::new(ping::Config::new()),
            identify: identify::Behaviour::new(identify::Config::new(
                "/TODO/0.0.1".to_string(),
                key.public(),
            )),
        })?
        .build();

    // Listen on all interfaces
    let listen_addr_tcp = Multiaddr::empty()
        .with(match opt.use_ipv6 {
            Some(true) => Protocol::from(Ipv6Addr::UNSPECIFIED),
            _ => Protocol::from(Ipv4Addr::UNSPECIFIED),
        })
        .with(Protocol::Tcp(opt.port));
    swarm.listen_on(listen_addr_tcp)?;

    let listen_addr_quic = Multiaddr::empty()
        .with(match opt.use_ipv6 {
            Some(true) => Protocol::from(Ipv6Addr::UNSPECIFIED),
            _ => Protocol::from(Ipv4Addr::UNSPECIFIED),
        })
        .with(Protocol::Udp(opt.port))
        .with(Protocol::QuicV1);
    swarm.listen_on(listen_addr_quic)?;

    listen_on_websocket(&mut swarm, &opt)?;
    listen_on_webrtc(&mut swarm, &opt)?;

    loop {
        match swarm.next().await.expect("Infinite Stream.") {
            SwarmEvent::Behaviour(event) => {
                if let BehaviourEvent::Identify(identify::Event::Received {
                    info: identify::Info { observed_addr, .. },
                    ..
                }) = &event
                {
                    swarm.add_external_address(observed_addr.clone());
                }

                println!("{event:?}")
            }
            SwarmEvent::NewListenAddr { mut address, .. } => {
                address.push(Protocol::P2p(local_peer_id.into()));
                println!("Listening on {address:?}");
            }
            _ => {}
        }
    }
}

fn listen_on_websocket(swarm: &mut Swarm<Behaviour>, opt: &Opt) -> Result<(), Box<dyn Error>> {
    match opt.websocket_port {
        Some(0) => {
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput,
            "Websocket port is 0, which is not supported in this example, since it will use a non-deterministic port. Please use a fixed port for websocket.")));
        }
        Some(port) => {
            let address = Multiaddr::from(Ipv4Addr::UNSPECIFIED)
                .with(Protocol::Tcp(port))
                .with(Protocol::Ws(std::borrow::Cow::Borrowed("/")));

            info!(?address, "Listening on webSocket");
            swarm.listen_on(address.clone())?;
        }
        None => {
            info!("Does not use websocket");
        }
    }
    Ok(())
}

fn listen_on_webrtc(swarm: &mut Swarm<Behaviour>, opt: &Opt) -> Result<(), Box<dyn Error>> {
    match opt.webrtc_port {
        Some(0) => {
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput,
            "Websocket port is 0, which is not supported in this example, since it will use a non-deterministic port. Please use a fixed port for webRTC.")));
        }
        Some(port) => {
            let address = Multiaddr::from(Ipv4Addr::UNSPECIFIED)
                .with(Protocol::Udp(port))
                .with(Protocol::WebRTCDirect);

            info!(?address, "Listening on webRTC");
            swarm.listen_on(address.clone())?;
        }
        None => {
            info!("Does not use webRTC");
        }
    }
    Ok(())
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
}

fn generate_ed25519(secret_key_seed: u8) -> identity::Keypair {
    let mut bytes = [0u8; 32];
    bytes[0] = secret_key_seed;

    identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
}

#[derive(Debug, Parser)]
#[command(name = "libp2p relay")]
struct Opt {
    /// Determine if the relay listen on ipv6 or ipv4 loopback address. the default is ipv4
    #[arg(long)]
    use_ipv6: Option<bool>,

    /// Fixed value to generate deterministic peer id
    #[arg(long)]
    secret_key_seed: u8,

    /// The port used to listen on all interfaces
    #[arg(long)]
    port: u16,

    /// The websocket port used to listen on all interfaces
    #[arg(long)]
    websocket_port: Option<u16>,

    /// The webrtc port used to listen on all interfaces
    #[arg(long)]
    webrtc_port: Option<u16>,
}
