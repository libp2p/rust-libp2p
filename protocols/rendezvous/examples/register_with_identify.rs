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
use libp2p::core::identity;
use libp2p::core::PeerId;
use libp2p::identify::{Identify, IdentifyConfig, IdentifyEvent};
use libp2p::ping::{Ping, PingConfig, PingEvent, PingSuccess};
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::{development_transport, rendezvous};
use libp2p::{Multiaddr, NetworkBehaviour};
use std::time::Duration;

#[tokio::main]
async fn main() {
    env_logger::init();

    let rendezvous_point_address = "/ip4/127.0.0.1/tcp/62649".parse::<Multiaddr>().unwrap();
    let rendezvous_point = "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
        .parse()
        .unwrap();

    let identity = identity::Keypair::generate_ed25519();

    let mut swarm = Swarm::new(
        development_transport(identity.clone()).await.unwrap(),
        MyBehaviour {
            identify: Identify::new(IdentifyConfig::new(
                "rendezvous-example/1.0.0".to_string(),
                identity.public(),
            )),
            rendezvous: rendezvous::client::Behaviour::new(identity.clone()),
            ping: Ping::new(
                PingConfig::new()
                    .with_interval(Duration::from_secs(1))
                    .with_keep_alive(true),
            ),
        },
        PeerId::from(identity.public()),
    );

    log::info!("Local peer id: {}", swarm.local_peer_id());

    let _ = swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap());

    swarm.dial(rendezvous_point_address).unwrap();

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
            // once `/identify` did its job, we know our external address and can register
            SwarmEvent::Behaviour(MyEvent::Identify(IdentifyEvent::Received { .. })) => {
                swarm.behaviour_mut().rendezvous.register(
                    rendezvous::Namespace::from_static("rendezvous"),
                    rendezvous_point,
                    None,
                );
            }
            SwarmEvent::Behaviour(MyEvent::Rendezvous(rendezvous::client::Event::Registered {
                namespace,
                ttl,
                rendezvous_node,
            })) => {
                log::info!(
                    "Registered for namespace '{}' at rendezvous point {} for the next {} seconds",
                    namespace,
                    rendezvous_node,
                    ttl
                );
            }
            SwarmEvent::Behaviour(MyEvent::Rendezvous(
                rendezvous::client::Event::RegisterFailed(error),
            )) => {
                log::error!("Failed to register {}", error);
                return;
            }
            SwarmEvent::Behaviour(MyEvent::Ping(PingEvent {
                peer,
                result: Ok(PingSuccess::Ping { rtt }),
            })) if peer != rendezvous_point => {
                log::info!("Ping to {} is {}ms", peer, rtt.as_millis())
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
    Rendezvous(rendezvous::client::Event),
    Identify(IdentifyEvent),
    Ping(PingEvent),
}

impl From<rendezvous::client::Event> for MyEvent {
    fn from(event: rendezvous::client::Event) -> Self {
        MyEvent::Rendezvous(event)
    }
}

impl From<IdentifyEvent> for MyEvent {
    fn from(event: IdentifyEvent) -> Self {
        MyEvent::Identify(event)
    }
}

impl From<PingEvent> for MyEvent {
    fn from(event: PingEvent) -> Self {
        MyEvent::Ping(event)
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(event_process = false)]
#[behaviour(out_event = "MyEvent")]
struct MyBehaviour {
    identify: Identify,
    rendezvous: rendezvous::client::Behaviour,
    ping: Ping,
}
