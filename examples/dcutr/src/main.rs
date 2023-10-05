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

#![doc = include_str!("../README.md")]

use clap::Parser;
use futures::{
    executor::{block_on, ThreadPool},
    future::{Either, FutureExt},
    stream::StreamExt,
};
use libp2p::{
    core::{
        multiaddr::{Multiaddr, Protocol},
        muxing::StreamMuxerBox,
        transport::Transport,
        upgrade,
    },
    dcutr, dns, identify, identity, noise, ping, quic, relay,
    swarm::{NetworkBehaviour, SwarmBuilder, SwarmEvent},
    tcp, yamux, PeerId,
};
use std::error::Error;
use std::str::FromStr;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[clap(name = "libp2p DCUtR client")]
struct Opts {
    /// The mode (client-listen, client-dial).
    #[clap(long)]
    mode: Mode,

    /// Fixed value to generate deterministic peer id.
    #[clap(long)]
    secret_key_seed: u8,

    /// The listening address
    #[clap(long)]
    relay_address: Multiaddr,

    /// Peer ID of the remote peer to hole punch to.
    #[clap(long)]
    remote_peer_id: Option<PeerId>,
}

#[derive(Clone, Debug, PartialEq, Parser)]
enum Mode {
    Dial,
    Listen,
}

impl FromStr for Mode {
    type Err = String;
    fn from_str(mode: &str) -> Result<Self, Self::Err> {
        match mode {
            "dial" => Ok(Mode::Dial),
            "listen" => Ok(Mode::Listen),
            _ => Err("Expected either 'dial' or 'listen'".to_string()),
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let opts = Opts::parse();

    let local_key = generate_ed25519(opts.secret_key_seed);
    let local_peer_id = PeerId::from(local_key.public());

    let (relay_transport, client) = relay::client::new(local_peer_id);

    let transport = {
        let relay_tcp_quic_transport = relay_transport
            .or_transport(tcp::async_io::Transport::new(
                tcp::Config::default().port_reuse(true),
            ))
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::Config::new(&local_key).unwrap())
            .multiplex(yamux::Config::default())
            .or_transport(quic::async_std::Transport::new(quic::Config::new(
                &local_key,
            )));

        block_on(dns::async_std::Transport::system(relay_tcp_quic_transport))
            .unwrap()
            .map(|either_output, _| match either_output {
                Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
                Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            })
            .boxed()
    };

    #[derive(NetworkBehaviour)]
    #[behaviour(to_swarm = "Event")]
    struct Behaviour {
        relay_client: relay::client::Behaviour,
        ping: ping::Behaviour,
        identify: identify::Behaviour,
        dcutr: dcutr::Behaviour,
    }

    #[derive(Debug)]
    #[allow(clippy::large_enum_variant)]
    enum Event {
        Ping(ping::Event),
        Identify(identify::Event),
        Relay(relay::client::Event),
        Dcutr(dcutr::Event),
    }

    impl From<ping::Event> for Event {
        fn from(e: ping::Event) -> Self {
            Event::Ping(e)
        }
    }

    impl From<identify::Event> for Event {
        fn from(e: identify::Event) -> Self {
            Event::Identify(e)
        }
    }

    impl From<relay::client::Event> for Event {
        fn from(e: relay::client::Event) -> Self {
            Event::Relay(e)
        }
    }

    impl From<dcutr::Event> for Event {
        fn from(e: dcutr::Event) -> Self {
            Event::Dcutr(e)
        }
    }

    let behaviour = Behaviour {
        relay_client: client,
        ping: ping::Behaviour::new(ping::Config::new()),
        identify: identify::Behaviour::new(identify::Config::new(
            "/TODO/0.0.1".to_string(),
            local_key.public(),
        )),
        dcutr: dcutr::Behaviour::new(local_peer_id),
    };

    let mut swarm = match ThreadPool::new() {
        Ok(tp) => SwarmBuilder::with_executor(transport, behaviour, local_peer_id, tp),
        Err(_) => SwarmBuilder::without_executor(transport, behaviour, local_peer_id),
    }
    .build();

    swarm
        .listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse().unwrap())
        .unwrap();
    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .unwrap();

    // Wait to listen on all interfaces.
    block_on(async {
        let mut delay = futures_timer::Delay::new(std::time::Duration::from_secs(1)).fuse();
        loop {
            futures::select! {
                event = swarm.next() => {
                    match event.unwrap() {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            tracing::info!(%address, "Listening on address");
                        }
                        event => panic!("{event:?}"),
                    }
                }
                _ = delay => {
                    // Likely listening on all interfaces now, thus continuing by breaking the loop.
                    break;
                }
            }
        }
    });

    // Connect to the relay server. Not for the reservation or relayed connection, but to (a) learn
    // our local public address and (b) enable a freshly started relay to learn its public address.
    swarm.dial(opts.relay_address.clone()).unwrap();
    block_on(async {
        let mut learned_observed_addr = false;
        let mut told_relay_observed_addr = false;

        loop {
            match swarm.next().await.unwrap() {
                SwarmEvent::NewListenAddr { .. } => {}
                SwarmEvent::Dialing { .. } => {}
                SwarmEvent::ConnectionEstablished { .. } => {}
                SwarmEvent::Behaviour(Event::Ping(_)) => {}
                SwarmEvent::Behaviour(Event::Identify(identify::Event::Sent { .. })) => {
                    tracing::info!("Told relay its public address");
                    told_relay_observed_addr = true;
                }
                SwarmEvent::Behaviour(Event::Identify(identify::Event::Received {
                    info: identify::Info { observed_addr, .. },
                    ..
                })) => {
                    tracing::info!(address=%observed_addr, "Relay told us our public address");
                    swarm.add_external_address(observed_addr);
                    learned_observed_addr = true;
                }
                event => panic!("{event:?}"),
            }

            if learned_observed_addr && told_relay_observed_addr {
                break;
            }
        }
    });

    match opts.mode {
        Mode::Dial => {
            swarm
                .dial(
                    opts.relay_address
                        .with(Protocol::P2pCircuit)
                        .with(Protocol::P2p(opts.remote_peer_id.unwrap())),
                )
                .unwrap();
        }
        Mode::Listen => {
            swarm
                .listen_on(opts.relay_address.with(Protocol::P2pCircuit))
                .unwrap();
        }
    }

    block_on(async {
        loop {
            match swarm.next().await.unwrap() {
                SwarmEvent::NewListenAddr { address, .. } => {
                    tracing::info!(%address, "Listening on address");
                }
                SwarmEvent::Behaviour(Event::Relay(
                    relay::client::Event::ReservationReqAccepted { .. },
                )) => {
                    assert!(opts.mode == Mode::Listen);
                    tracing::info!("Relay accepted our reservation request");
                }
                SwarmEvent::Behaviour(Event::Relay(event)) => {
                    tracing::info!(?event)
                }
                SwarmEvent::Behaviour(Event::Dcutr(event)) => {
                    tracing::info!(?event)
                }
                SwarmEvent::Behaviour(Event::Identify(event)) => {
                    tracing::info!(?event)
                }
                SwarmEvent::Behaviour(Event::Ping(_)) => {}
                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } => {
                    tracing::info!(peer=%peer_id, ?endpoint, "Established new connection");
                }
                SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                    tracing::info!(peer=?peer_id, "Outgoing connection failed: {error}");
                }
                _ => {}
            }
        }
    })
}

fn generate_ed25519(secret_key_seed: u8) -> identity::Keypair {
    let mut bytes = [0u8; 32];
    bytes[0] = secret_key_seed;

    identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
}
