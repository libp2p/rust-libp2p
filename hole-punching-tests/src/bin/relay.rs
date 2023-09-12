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

use anyhow::Context;
use clap::Parser;
use futures::future::Either;
use futures::stream::StreamExt;
use libp2p::{
    core::multiaddr::Protocol,
    core::muxing::StreamMuxerBox,
    core::upgrade,
    core::{Multiaddr, Transport},
    identify, identity,
    identity::PeerId,
    noise, ping, quic, relay,
    swarm::{NetworkBehaviour, SwarmBuilder, SwarmEvent},
    tcp, yamux,
};
use log::{info, LevelFilter};
use redis::AsyncCommands;
use std::error::Error;
use std::net::IpAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .parse_default_env()
        .init();

    let opt = Opt::parse();

    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    info!("Local peer id: {local_peer_id}");

    let tcp_transport = tcp::tokio::Transport::default()
        .upgrade(upgrade::Version::V1Lazy)
        .authenticate(
            noise::Config::new(&local_key).expect("Signing libp2p-noise static DH keypair failed."),
        )
        .multiplex(yamux::Config::default());

    let transport = quic::tokio::Transport::new(quic::Config::new(&local_key))
        .or_transport(tcp_transport)
        .map(|either_output, _| match either_output {
            Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
        })
        .boxed();

    let behaviour = Behaviour {
        relay: relay::Behaviour::new(local_peer_id, Default::default()),
        ping: ping::Behaviour::new(ping::Config::new()),
        identify: identify::Behaviour::new(identify::Config::new(
            "/TODO/0.0.1".to_string(),
            local_key.public(),
        )),
    };

    let mut swarm = SwarmBuilder::with_tokio_executor(transport, behaviour, local_peer_id).build();

    // Listen on all interfaces
    let listen_addr_tcp = Multiaddr::empty()
        .with(opt.listen_addr.into())
        .with(Protocol::Tcp(0));
    let tcp_listener_id = swarm.listen_on(listen_addr_tcp)?;

    let listen_addr_quic = Multiaddr::empty()
        .with(opt.listen_addr.into())
        .with(Protocol::Udp(0))
        .with(Protocol::QuicV1);
    let quic_listener_id = swarm.listen_on(listen_addr_quic)?;

    let client = redis::Client::open("redis://redis:6379/")?;
    let mut connection = client
        .get_async_connection()
        .await
        .context("Failed to connect to redis server")?;

    loop {
        match swarm.next().await.expect("Infinite Stream.") {
            SwarmEvent::NewListenAddr {
                address,
                listener_id,
            } => {
                // swarm.add_external_address(address); // We know that in our testing network setup, that we are listening on a "publicly-reachable" address.

                info!("Listening on {address}");

                let address = address
                    .with(Protocol::P2p(*swarm.local_peer_id()))
                    .to_string();

                // Push each address twice because we need to connect two clients.

                if listener_id == tcp_listener_id {
                    connection.rpush("RELAY_TCP_ADDRESS", &address).await?;
                    connection.rpush("RELAY_TCP_ADDRESS", &address).await?;
                }
                if listener_id == quic_listener_id {
                    connection.rpush("RELAY_QUIC_ADDRESS", &address).await?;
                    connection.rpush("RELAY_QUIC_ADDRESS", &address).await?;
                }
            }
            _ => {}
        }
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay: relay::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
}

#[derive(Debug, Parser)]
#[clap(name = "libp2p relay")]
struct Opt {
    /// Which local address to listen on
    #[clap(long)]
    listen_addr: IpAddr,
}
