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
use libp2p::{
    core::transport::upgrade::Version,
    identity, noise, ping, rendezvous,
    swarm::{keep_alive, NetworkBehaviour, SwarmBuilder, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Transport,
};
use std::time::Duration;

#[tokio::main]
async fn main() {
    env_logger::init();

    let key_pair = identity::Keypair::generate_ed25519();
    let rendezvous_point_address = "/ip4/127.0.0.1/tcp/62649".parse::<Multiaddr>().unwrap();
    let rendezvous_point = "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
        .parse()
        .unwrap();

    let mut swarm = SwarmBuilder::with_tokio_executor(
        tcp::tokio::Transport::default()
            .upgrade(Version::V1Lazy)
            .authenticate(noise::Config::new(&key_pair).unwrap())
            .multiplex(yamux::Config::default())
            .boxed(),
        MyBehaviour {
            rendezvous: rendezvous::client::Behaviour::new(key_pair.clone()),
            ping: ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(1))),
            keep_alive: keep_alive::Behaviour,
        },
        PeerId::from(key_pair.public()),
    )
    .build();

    // In production the external address should be the publicly facing IP address of the rendezvous point.
    // This address is recorded in the registration entry by the rendezvous point.
    let external_address = "/ip4/127.0.0.1/tcp/0".parse::<Multiaddr>().unwrap();
    swarm.add_external_address(external_address);

    swarm.dial(rendezvous_point_address.clone()).unwrap();

    while let Some(event) = swarm.next().await {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                log::info!("Listening on {}", address);
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                cause: Some(error),
                ..
            } if peer_id == rendezvous_point => {
                log::error!("Lost connection to rendezvous point {}", error);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == rendezvous_point => {
                if let Err(error) = swarm.behaviour_mut().rendezvous.register(
                    rendezvous::Namespace::from_static("rendezvous"),
                    rendezvous_point,
                    None,
                ) {
                    log::error!("Failed to register: {error}");
                    return;
                }
                log::info!("Connection established with rendezvous point {}", peer_id);
            }
            // once `/identify` did its job, we know our external address and can register
            SwarmEvent::Behaviour(MyBehaviourEvent::Rendezvous(
                rendezvous::client::Event::Registered {
                    namespace,
                    ttl,
                    rendezvous_node,
                },
            )) => {
                log::info!(
                    "Registered for namespace '{}' at rendezvous point {} for the next {} seconds",
                    namespace,
                    rendezvous_node,
                    ttl
                );
            }
            SwarmEvent::Behaviour(MyBehaviourEvent::Rendezvous(
                rendezvous::client::Event::RegisterFailed {
                    rendezvous_node,
                    namespace,
                    error,
                },
            )) => {
                log::error!(
                    "Failed to register: rendezvous_node={}, namespace={}, error_code={:?}",
                    rendezvous_node,
                    namespace,
                    error
                );
                return;
            }
            SwarmEvent::Behaviour(MyBehaviourEvent::Ping(ping::Event {
                peer,
                result: Ok(rtt),
                ..
            })) if peer != rendezvous_point => {
                log::info!("Ping to {} is {}ms", peer, rtt.as_millis())
            }
            other => {
                log::debug!("Unhandled {:?}", other);
            }
        }
    }
}

#[derive(NetworkBehaviour)]
struct MyBehaviour {
    rendezvous: rendezvous::client::Behaviour,
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
}
