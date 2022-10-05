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

use async_trait::async_trait;
use futures::stream::FusedStream;
use futures::StreamExt;
use futures::{future, Stream};
use libp2p::core::transport::upgrade::Version;
use libp2p::core::transport::MemoryTransport;
use libp2p::core::upgrade::SelectUpgrade;
use libp2p::core::{identity, Multiaddr, PeerId, Transport};
use libp2p::mplex::MplexConfig;
use libp2p::noise::NoiseAuthenticated;
use libp2p::swarm::{AddressScore, NetworkBehaviour, Swarm, SwarmBuilder, SwarmEvent};
use libp2p::yamux::YamuxConfig;
use std::fmt::Debug;
use std::time::Duration;

pub fn new_swarm<B, F>(behaviour_fn: F) -> Swarm<B>
where
    B: NetworkBehaviour,
    <B as NetworkBehaviour>::OutEvent: Debug,
    B: NetworkBehaviour,
    F: FnOnce(PeerId, identity::Keypair) -> B,
{
    let identity = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(identity.public());

    let transport = MemoryTransport::default()
        .upgrade(Version::V1)
        .authenticate(NoiseAuthenticated::xx(&identity).unwrap())
        .multiplex(SelectUpgrade::new(
            YamuxConfig::default(),
            MplexConfig::new(),
        ))
        .timeout(Duration::from_secs(5))
        .boxed();

    SwarmBuilder::new(transport, behaviour_fn(peer_id, identity), peer_id)
        .executor(Box::new(|future| {
            let _ = tokio::spawn(future);
        }))
        .build()
}

fn get_rand_memory_address() -> Multiaddr {
    let address_port = rand::random::<u64>();

    format!("/memory/{}", address_port)
        .parse::<Multiaddr>()
        .unwrap()
}

pub async fn await_event_or_timeout<Event, Error>(
    swarm: &mut (impl Stream<Item = SwarmEvent<Event, Error>> + FusedStream + Unpin),
) -> SwarmEvent<Event, Error>
where
    SwarmEvent<Event, Error>: Debug,
{
    tokio::time::timeout(
        Duration::from_secs(30),
        swarm
            .inspect(|event| log::debug!("Swarm emitted {:?}", event))
            .select_next_some(),
    )
    .await
    .expect("network behaviour to emit an event within 30 seconds")
}

pub async fn await_events_or_timeout<Event1, Event2, Error1, Error2>(
    swarm_1: &mut (impl Stream<Item = SwarmEvent<Event1, Error1>> + FusedStream + Unpin),
    swarm_2: &mut (impl Stream<Item = SwarmEvent<Event2, Error2>> + FusedStream + Unpin),
) -> (SwarmEvent<Event1, Error1>, SwarmEvent<Event2, Error2>)
where
    SwarmEvent<Event1, Error1>: Debug,
    SwarmEvent<Event2, Error2>: Debug,
{
    tokio::time::timeout(
        Duration::from_secs(30),
        future::join(
            swarm_1
                .inspect(|event| log::debug!("Swarm1 emitted {:?}", event))
                .select_next_some(),
            swarm_2
                .inspect(|event| log::debug!("Swarm2 emitted {:?}", event))
                .select_next_some(),
        ),
    )
    .await
    .expect("network behaviours to emit an event within 30 seconds")
}

#[macro_export]
macro_rules! assert_behaviour_events {
    ($swarm: ident: $pat: pat, || $body: block) => {
        match await_event_or_timeout(&mut $swarm).await {
            libp2p::swarm::SwarmEvent::Behaviour($pat) => $body,
            _ => panic!("Unexpected combination of events emitted, check logs for details"),
        }
    };
    ($swarm1: ident: $pat1: pat, $swarm2: ident: $pat2: pat, || $body: block) => {
        match await_events_or_timeout(&mut $swarm1, &mut $swarm2).await {
            (
                libp2p::swarm::SwarmEvent::Behaviour($pat1),
                libp2p::swarm::SwarmEvent::Behaviour($pat2),
            ) => $body,
            _ => panic!("Unexpected combination of events emitted, check logs for details"),
        }
    };
}

/// An extension trait for [`Swarm`] that makes it easier to set up a network of [`Swarm`]s for tests.
#[async_trait]
pub trait SwarmExt {
    /// Establishes a connection to the given [`Swarm`], polling both of them until the connection is established.
    async fn block_on_connection<T>(&mut self, other: &mut Swarm<T>)
    where
        T: NetworkBehaviour + Send,
        <T as NetworkBehaviour>::OutEvent: Debug;

    /// Listens on a random memory address, polling the [`Swarm`] until the transport is ready to accept connections.
    async fn listen_on_random_memory_address(&mut self) -> Multiaddr;

    /// Spawns the given [`Swarm`] into a runtime, polling it endlessly.
    fn spawn_into_runtime(self);
}

#[async_trait]
impl<B> SwarmExt for Swarm<B>
where
    B: NetworkBehaviour + Send,
    <B as NetworkBehaviour>::OutEvent: Debug,
{
    async fn block_on_connection<T>(&mut self, other: &mut Swarm<T>)
    where
        T: NetworkBehaviour + Send,
        <T as NetworkBehaviour>::OutEvent: Debug,
    {
        let addr_to_dial = other.external_addresses().next().unwrap().addr.clone();

        self.dial(addr_to_dial.clone()).unwrap();

        let mut dialer_done = false;
        let mut listener_done = false;

        loop {
            let dialer_event_fut = self.select_next_some();

            tokio::select! {
                dialer_event = dialer_event_fut => {
                    match dialer_event {
                        SwarmEvent::ConnectionEstablished { .. } => {
                            dialer_done = true;
                        }
                        other => {
                            log::debug!("Ignoring {:?}", other);
                        }
                    }
                },
                listener_event = other.select_next_some() => {
                    match listener_event {
                        SwarmEvent::ConnectionEstablished { .. } => {
                            listener_done = true;
                        }
                        SwarmEvent::IncomingConnectionError { error, .. } => {
                            panic!("Failure in incoming connection {}", error);
                        }
                        other => {
                            log::debug!("Ignoring {:?}", other);
                        }
                    }
                }
            }

            if dialer_done && listener_done {
                return;
            }
        }
    }

    async fn listen_on_random_memory_address(&mut self) -> Multiaddr {
        let memory_addr_listener_id = self.listen_on(get_rand_memory_address()).unwrap();

        // block until we are actually listening
        let multiaddr = loop {
            match self.select_next_some().await {
                SwarmEvent::NewListenAddr {
                    address,
                    listener_id,
                } if listener_id == memory_addr_listener_id => {
                    break address;
                }
                other => {
                    log::debug!(
                        "Ignoring {:?} while waiting for listening to succeed",
                        other
                    );
                }
            }
        };

        // Memory addresses are externally reachable because they all share the same memory-space.
        self.add_external_address(multiaddr.clone(), AddressScore::Infinite);

        multiaddr
    }

    fn spawn_into_runtime(mut self) {
        tokio::spawn(async move {
            loop {
                self.next().await;
            }
        });
    }
}
