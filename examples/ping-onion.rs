// Copyright 2022 Hannes Furmans
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

//! Ping-Onion example
//!
//! See ../src/tutorial.rs for a step-by-step guide building the example below.
//!
//! This example requires two seperate computers, one of which has to be reachable from the
//! internet.
//!
//! On the first computer run:
//! ```sh
//! cargo run --example ping
//! ```
//!
//! It will print the PeerId and the listening addresses, e.g. `Listening on
//! "/ip4/0.0.0.0/tcp/24915"`
//!
//! Make sure that the first computer is reachable under one of these ip addresses and port.
//!
//! On the second computer run:
//! ```sh
//! cargo run --example ping-onion -- /ip4/123.45.67.89/tcp/24915
//! ```
//!
//! The two nodes establish a connection, negotiate the ping protocol
//! and begin pinging each other over Tor.

use futures::prelude::*;
use libp2p::onion::AddressConversion;
use libp2p::swarm::{keep_alive, Swarm, SwarmEvent};
use libp2p::{
    core::upgrade, identity, mplex, noise, onion, ping, yamux, Multiaddr, NetworkBehaviour, PeerId,
    Transport,
};
use std::error::Error;

async fn onion_transport(
    keypair: identity::Keypair,
) -> Result<
    libp2p_core::transport::Boxed<(PeerId, libp2p_core::muxing::StreamMuxerBox)>,
    Box<dyn Error>,
> {
    use std::time::Duration;

    let transport = onion::AsyncStdNativeTlsOnionTransport::bootstrapped()
        .await?
        .with_address_conversion(AddressConversion::IpAndDns);
    Ok(transport
        .upgrade(upgrade::Version::V1)
        .authenticate(
            noise::NoiseAuthenticated::xx(&keypair)
                .expect("Signing libp2p-noise static DH keypair failed."),
        )
        .multiplex(upgrade::SelectUpgrade::new(
            yamux::YamuxConfig::default(),
            mplex::MplexConfig::default(),
        ))
        .timeout(Duration::from_secs(20))
        .boxed())
}

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let addr = std::env::args().nth(1).expect("no multiaddr given");
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);

    let transport = onion_transport(local_key).await?;

    let mut swarm = Swarm::new(transport, Behaviour::default(), local_peer_id);

    // Dial the peer identified by the multi-address given as the second
    // command-line argument, if any.
    let remote: Multiaddr = addr.parse()?;
    swarm.dial(remote)?;
    println!("Dialed {}", addr);

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::ConnectionEstablished { endpoint, .. } => {
                let endpoint_addr = endpoint.get_remote_address();
                println!("Connection established to {:?}", endpoint_addr);
            }
            SwarmEvent::OutgoingConnectionError { error, .. } => {
                println!("Error establishing outgoing connection: {:?}", error)
            }
            SwarmEvent::Behaviour(event) => println!("{:?}", event),
            _ => {}
        }
    }
}

/// Our network behaviour.
///
/// For illustrative purposes, this includes the [`KeepAlive`](behaviour::KeepAlive) behaviour so a continuous sequence of
/// pings can be observed.
#[derive(NetworkBehaviour, Default)]
struct Behaviour {
    keep_alive: keep_alive::Behaviour,
    ping: ping::Behaviour,
}
