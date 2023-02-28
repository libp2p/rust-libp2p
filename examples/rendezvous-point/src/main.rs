// Copyright 2021 COMIT Network.
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

/// Examples for the rendezvous protocol:
///
/// 1. Run the rendezvous server:
///     ```
///     cd rendezvous-point
///     RUST_LOG=info cargo run
///     ```
/// 2. Register a peer:
///     ```
///     cd rendezvous-register
///     RUST_LOG=info cargo run
///     ```
/// 3. Try to discover the peer from (2):
///     ```
///     cd rendezvous-discover
///     RUST_LOG=info cargo run
///     ```
use futures::StreamExt;
use libp2p::{
    core::transport::upgrade::Version,
    identify, identity, ping, rendezvous,
    swarm::{derive_prelude, keep_alive, NetworkBehaviour, Swarm, SwarmEvent},
    PeerId, Transport,
};
use std::time::Duration;
use void::Void;

#[tokio::main]
async fn main() {
    env_logger::init();

    let key_pair = identity::Keypair::generate_ed25519();

    let mut swarm = Swarm::with_tokio_executor(
        libp2p::tcp::tokio::Transport::default()
            .upgrade(Version::V1)
            .authenticate(libp2p::noise::NoiseAuthenticated::xx(&key_pair).unwrap())
            .multiplex(libp2p::yamux::YamuxConfig::default())
            .boxed(),
        MyBehaviour {
            identify: identify::Behaviour::new(identify::Config::new(
                "rendezvous-example/1.0.0".to_string(),
                key_pair.public(),
            )),
            rendezvous: rendezvous::server::Behaviour::new(rendezvous::server::Config::default()),
            ping: ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(1))),
            keep_alive: keep_alive::Behaviour,
        },
        PeerId::from(key_pair.public()),
    );

    log::info!("Local peer id: {}", swarm.local_peer_id());

    let _ = swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap());

    while let Some(event) = swarm.next().await {
        match event {
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                log::info!("Connected to {}", peer_id);
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                log::info!("Disconnected from {}", peer_id);
            }
            SwarmEvent::Behaviour(MyEvent::Rendezvous(
                rendezvous::server::Event::PeerRegistered { peer, registration },
            )) => {
                log::info!(
                    "Peer {} registered for namespace '{}'",
                    peer,
                    registration.namespace
                );
            }
            SwarmEvent::Behaviour(MyEvent::Rendezvous(
                rendezvous::server::Event::DiscoverServed {
                    enquirer,
                    registrations,
                },
            )) => {
                log::info!(
                    "Served peer {} with {} registrations",
                    enquirer,
                    registrations.len()
                );
            }
            other => {
                log::debug!("Unhandled {:?}", other);
            }
        }
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
enum MyEvent {
    Rendezvous(rendezvous::server::Event),
    Identify(identify::Event),
    Ping(ping::Event),
}

impl From<rendezvous::server::Event> for MyEvent {
    fn from(event: rendezvous::server::Event) -> Self {
        MyEvent::Rendezvous(event)
    }
}

impl From<identify::Event> for MyEvent {
    fn from(event: identify::Event) -> Self {
        MyEvent::Identify(event)
    }
}

impl From<ping::Event> for MyEvent {
    fn from(event: ping::Event) -> Self {
        MyEvent::Ping(event)
    }
}

impl From<Void> for MyEvent {
    fn from(event: Void) -> Self {
        void::unreachable(event)
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(
    out_event = "MyEvent",
    event_process = false,
    prelude = "derive_prelude"
)]
struct MyBehaviour {
    identify: identify::Behaviour,
    rendezvous: rendezvous::server::Behaviour,
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
}
