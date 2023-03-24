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

use clap::Parser;
use futures::executor::block_on;
use futures::stream::StreamExt;
use libp2p::{
    core::multiaddr::Protocol,
    core::upgrade,
    core::{Multiaddr, Transport},
    identify, identity,
    identity::PeerId,
    noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmBuilder, SwarmEvent},
    tcp,
};
use std::error::Error;
use std::net::{Ipv4Addr, Ipv6Addr};

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let opt = Opt::parse();
    println!("opt: {opt:?}");

    // Create a static known PeerId based on given secret
    let local_key: identity::Keypair = generate_ed25519(opt.secret_key_seed);
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {local_peer_id:?}");

    let tcp_transport = tcp::async_io::Transport::default();

    let transport = tcp_transport
        .upgrade(upgrade::Version::V1)
        .authenticate(
            noise::NoiseAuthenticated::xx(&local_key)
                .expect("Signing libp2p-noise static DH keypair failed."),
        )
        .multiplex(libp2p::yamux::YamuxConfig::default())
        .boxed();

    let behaviour = Behaviour {
        relay: relay::Behaviour::new(local_peer_id, Default::default()),
        ping: ping::Behaviour::new(ping::Config::new()),
        identify: identify::Behaviour::new(identify::Config::new(
            "/TODO/0.0.1".to_string(),
            local_key.public(),
        )),
    };

    let mut swarm = SwarmBuilder::without_executor(transport, behaviour, local_peer_id).build();

    // Listen on all interfaces
    let listen_addr = Multiaddr::empty()
        .with(match opt.use_ipv6 {
            Some(true) => Protocol::from(Ipv6Addr::UNSPECIFIED),
            _ => Protocol::from(Ipv4Addr::UNSPECIFIED),
        })
        .with(Protocol::Tcp(opt.port));
    swarm.listen_on(listen_addr)?;

    block_on(async {
        loop {
            match swarm.next().await.expect("Infinite Stream.") {
                SwarmEvent::Behaviour(event) => {
                    println!("{event:?}")
                }
                SwarmEvent::NewListenAddr { address, .. } => {
                    println!("Listening on {address:?}");
                }
                _ => {}
            }
        }
    })
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
#[clap(name = "libp2p relay")]
struct Opt {
    /// Determine if the relay listen on ipv6 or ipv4 loopback address. the default is ipv4
    #[clap(long)]
    use_ipv6: Option<bool>,

    /// Fixed value to generate deterministic peer id
    #[clap(long)]
    secret_key_seed: u8,

    /// The port used to listen on all interfaces
    #[clap(long)]
    port: u16,
}
