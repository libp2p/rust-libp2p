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

use futures::StreamExt;
use libp2p_core::{identity, upgrade::Version, PeerId, Transport};
use libp2p_identify as identify;
use libp2p_ping as ping;
use libp2p_rendezvous as rendezvous;
use libp2p_swarm::{keep_alive, NetworkBehaviour, Swarm, SwarmEvent};
use void::Void;

/// Examples for the rendezvous protocol:
///
/// 1. Run the rendezvous server:
///    RUST_LOG=info cargo run --example rendezvous_point
/// 2. Register a peer:
///    RUST_LOG=info cargo run --example register_with_identify
/// 3. Try to discover the peer from (2):
///    RUST_LOG=info cargo run --example discover
#[tokio::main]
async fn main() {
    env_logger::init();

    let bytes = [0u8; 32];
    let key = identity::ed25519::SecretKey::from_bytes(bytes).expect("we always pass 32 bytes");
    let identity = identity::Keypair::Ed25519(key.into());

    let mut swarm = Swarm::with_tokio_executor(
        libp2p_tcp::tokio::Transport::default()
            .upgrade(Version::V1)
            .authenticate(libp2p_noise::NoiseAuthenticated::xx(&identity).unwrap())
            .multiplex(libp2p_yamux::YamuxConfig::default())
            .boxed(),
        MyBehaviour {
            identify: identify::Behaviour::new(identify::Config::new(
                "rendezvous-example/1.0.0".to_string(),
                identity.public(),
            )),
            rendezvous: rendezvous::server::Behaviour::new(rendezvous::server::Config::default()),
            ping: ping::Behaviour::new(ping::Config::new()),
            keep_alive: keep_alive::Behaviour,
        },
        PeerId::from(identity.public()),
    );

    log::info!("Local peer id: {}", swarm.local_peer_id());

    swarm
        .listen_on("/ip4/0.0.0.0/tcp/62649".parse().unwrap())
        .unwrap();

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
enum MyEvent {
    Rendezvous(rendezvous::server::Event),
    Ping(ping::Event),
    Identify(identify::Event),
}

impl From<rendezvous::server::Event> for MyEvent {
    fn from(event: rendezvous::server::Event) -> Self {
        MyEvent::Rendezvous(event)
    }
}

impl From<ping::Event> for MyEvent {
    fn from(event: ping::Event) -> Self {
        MyEvent::Ping(event)
    }
}

impl From<identify::Event> for MyEvent {
    fn from(event: identify::Event) -> Self {
        MyEvent::Identify(event)
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
    prelude = "libp2p_swarm::derive_prelude"
)]
struct MyBehaviour {
    identify: identify::Behaviour,
    rendezvous: rendezvous::server::Behaviour,
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
}
