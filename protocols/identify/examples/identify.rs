// Copyright 2018 Parity Technologies (UK) Ltd.
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

//! identify example
//!
//! In the first terminal window, run:
//!
//! ```sh
//! cargo run --example identify
//! ```
//! It will print the [`PeerId`] and the listening addresses, e.g. `Listening on
//! "/ip4/127.0.0.1/tcp/24915"`
//!
//! In the second terminal window, start a new instance of the example with:
//!
//! ```sh
//! cargo run --example identify -- /ip4/127.0.0.1/tcp/24915
//! ```
//! The two nodes establish a connection, negotiate the identify protocol
//! and will send each other identify info which is then printed to the console.

use futures::prelude::*;
use libp2p_core::upgrade::Version;
use libp2p_core::{identity, Multiaddr, PeerId, Transport};
use libp2p_identify as identify;
use libp2p_swarm::{Swarm, SwarmEvent};
use std::error::Error;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {:?}", local_peer_id);

    let transport = libp2p_tcp::async_io::Transport::default()
        .upgrade(Version::V1)
        .authenticate(libp2p_noise::NoiseAuthenticated::xx(&local_key).unwrap())
        .multiplex(libp2p_yamux::YamuxConfig::default())
        .boxed();

    // Create a identify network behaviour.
    let behaviour = identify::Behaviour::new(identify::Config::new(
        "/ipfs/id/1.0.0".to_string(),
        local_key.public(),
    ));

    let mut swarm = Swarm::with_async_std_executor(transport, behaviour, local_peer_id);

    // Tell the swarm to listen on all interfaces and a random, OS-assigned
    // port.
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    // Dial the peer identified by the multi-address given as the second
    // command-line argument, if any.
    if let Some(addr) = std::env::args().nth(1) {
        let remote: Multiaddr = addr.parse()?;
        swarm.dial(remote)?;
        println!("Dialed {}", addr)
    }

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => println!("Listening on {:?}", address),
            // Prints peer id identify info is being sent to.
            SwarmEvent::Behaviour(identify::Event::Sent { peer_id, .. }) => {
                println!("Sent identify info to {:?}", peer_id)
            }
            // Prints out the info received via the identify event
            SwarmEvent::Behaviour(identify::Event::Received { info, .. }) => {
                println!("Received {:?}", info)
            }
            _ => {}
        }
    }
}
