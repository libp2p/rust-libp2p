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

//! Basic example for a AutoNAT server that supports the /libp2p/autonat/1.0.0 and "/ipfs/0.1.0" protocols.
//!
//! To start the server run:
//! ```sh
//! cargo run --example server -- --listen-port <port>
//! ```
//! The `listen-port` parameter is optional and allows to set a fixed port at which the local peer should listen.

use clap::Parser;
use futures::prelude::*;
use libp2p::autonat;
use libp2p::identify::{Identify, IdentifyConfig, IdentifyEvent};
use libp2p::multiaddr::Protocol;
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::{identity, Multiaddr, NetworkBehaviour, PeerId};
use std::error::Error;
use std::net::Ipv4Addr;

#[derive(Debug, Parser)]
#[clap(name = "libp2p autonat")]
struct Opt {
    #[clap(long)]
    listen_port: Option<u16>,
}

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let opt = Opt::parse();

    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);

    let transport = libp2p::development_transport(local_key.clone()).await?;

    let behaviour = Behaviour::new(local_key.public());

    let mut swarm = Swarm::new(transport, behaviour, local_peer_id);
    swarm.listen_on(
        Multiaddr::empty()
            .with(Protocol::Ip4(Ipv4Addr::UNSPECIFIED))
            .with(Protocol::Tcp(opt.listen_port.unwrap_or(0))),
    )?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => println!("Listening on {:?}", address),
            SwarmEvent::Behaviour(event) => println!("{:?}", event),
            e => println!("{:?}", e),
        }
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "Event")]
struct Behaviour {
    identify: Identify,
    auto_nat: autonat::Behaviour,
}

impl Behaviour {
    fn new(local_public_key: identity::PublicKey) -> Self {
        Self {
            identify: Identify::new(IdentifyConfig::new(
                "/ipfs/0.1.0".into(),
                local_public_key.clone(),
            )),
            auto_nat: autonat::Behaviour::new(
                local_public_key.to_peer_id(),
                autonat::Config::default(),
            ),
        }
    }
}

#[derive(Debug)]
enum Event {
    AutoNat(autonat::Event),
    Identify(IdentifyEvent),
}

impl From<IdentifyEvent> for Event {
    fn from(v: IdentifyEvent) -> Self {
        Self::Identify(v)
    }
}

impl From<autonat::Event> for Event {
    fn from(v: autonat::Event) -> Self {
        Self::AutoNat(v)
    }
}
