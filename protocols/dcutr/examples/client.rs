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
use futures::future::FutureExt;
use futures::stream::StreamExt;
use libp2p::core::multiaddr::{Multiaddr, Protocol};
use libp2p::core::transport::OrTransport;
use libp2p::core::upgrade;
use libp2p::dcutr;
use libp2p::dns::DnsConfig;
use libp2p::identify::{Identify, IdentifyConfig, IdentifyEvent, IdentifyInfo};
use libp2p::noise;
use libp2p::ping::{Ping, PingConfig, PingEvent};
use libp2p::relay::v2::client::{self, Client};
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::tcp::{GenTcpConfig, TcpTransport};
use libp2p::Transport;
use libp2p::{identity, NetworkBehaviour, PeerId};
use log::info;
use std::convert::TryInto;
use std::error::Error;
use std::net::Ipv4Addr;
use std::str::FromStr;

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

#[derive(Debug, Parser, PartialEq)]
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
    env_logger::init();

    let opts = Opts::parse();

    let local_key = generate_ed25519(opts.secret_key_seed);
    let local_peer_id = PeerId::from(local_key.public());
    info!("Local peer id: {:?}", local_peer_id);

    let (relay_transport, client) = Client::new_transport_and_behaviour(local_peer_id);

    let transport = OrTransport::new(
        relay_transport,
        block_on(DnsConfig::system(TcpTransport::new(
            GenTcpConfig::default().port_reuse(true),
        )))
        .unwrap(),
    )
    .upgrade(upgrade::Version::V1)
    .authenticate(
        noise::NoiseAuthenticated::xx(&local_key)
            .expect("Signing libp2p-noise static DH keypair failed."),
    )
    .multiplex(libp2p::yamux::YamuxConfig::default())
    .boxed();

    #[derive(NetworkBehaviour)]
    #[behaviour(out_event = "Event", event_process = false)]
    struct Behaviour {
        relay_client: Client,
        ping: Ping,
        identify: Identify,
        dcutr: dcutr::behaviour::Behaviour,
    }

    #[derive(Debug)]
    #[allow(clippy::large_enum_variant)]
    enum Event {
        Ping(PingEvent),
        Identify(IdentifyEvent),
        Relay(client::Event),
        Dcutr(dcutr::behaviour::Event),
    }

    impl From<PingEvent> for Event {
        fn from(e: PingEvent) -> Self {
            Event::Ping(e)
        }
    }

    impl From<IdentifyEvent> for Event {
        fn from(e: IdentifyEvent) -> Self {
            Event::Identify(e)
        }
    }

    impl From<client::Event> for Event {
        fn from(e: client::Event) -> Self {
            Event::Relay(e)
        }
    }

    impl From<dcutr::behaviour::Event> for Event {
        fn from(e: dcutr::behaviour::Event) -> Self {
            Event::Dcutr(e)
        }
    }

    let behaviour = Behaviour {
        relay_client: client,
        ping: Ping::new(PingConfig::new()),
        identify: Identify::new(IdentifyConfig::new(
            "/TODO/0.0.1".to_string(),
            local_key.public(),
        )),
        dcutr: dcutr::behaviour::Behaviour::new(),
    };

    let mut swarm = SwarmBuilder::new(transport, behaviour, local_peer_id)
        .dial_concurrency_factor(10_u8.try_into().unwrap())
        .build();

    swarm
        .listen_on(
            Multiaddr::empty()
                .with("0.0.0.0".parse::<Ipv4Addr>().unwrap().into())
                .with(Protocol::Tcp(0)),
        )
        .unwrap();

    // Wait to listen on all interfaces.
    block_on(async {
        let mut delay = futures_timer::Delay::new(std::time::Duration::from_secs(1)).fuse();
        loop {
            futures::select! {
                event = swarm.next() => {
                    match event.unwrap() {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            info!("Listening on {:?}", address);
                        }
                        event => panic!("{:?}", event),
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
                SwarmEvent::Behaviour(Event::Identify(IdentifyEvent::Sent { .. })) => {
                    info!("Told relay its public address.");
                    told_relay_observed_addr = true;
                }
                SwarmEvent::Behaviour(Event::Identify(IdentifyEvent::Received {
                    info: IdentifyInfo { observed_addr, .. },
                    ..
                })) => {
                    info!("Relay told us our public address: {:?}", observed_addr);
                    learned_observed_addr = true;
                }
                event => panic!("{:?}", event),
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
                        .with(Protocol::P2p(opts.remote_peer_id.unwrap().into())),
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
                    info!("Listening on {:?}", address);
                }
                SwarmEvent::Behaviour(Event::Relay(client::Event::ReservationReqAccepted {
                    ..
                })) => {
                    assert!(opts.mode == Mode::Listen);
                    info!("Relay accepted our reservation request.");
                }
                SwarmEvent::Behaviour(Event::Relay(event)) => {
                    info!("{:?}", event)
                }
                SwarmEvent::Behaviour(Event::Dcutr(event)) => {
                    info!("{:?}", event)
                }
                SwarmEvent::Behaviour(Event::Identify(event)) => {
                    info!("{:?}", event)
                }
                SwarmEvent::Behaviour(Event::Ping(_)) => {}
                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } => {
                    info!("Established connection to {:?} via {:?}", peer_id, endpoint);
                }
                SwarmEvent::OutgoingConnectionError { peer_id, error } => {
                    info!("Outgoing connection error to {:?}: {:?}", peer_id, error);
                }
                _ => {}
            }
        }
    })
}

fn generate_ed25519(secret_key_seed: u8) -> identity::Keypair {
    let mut bytes = [0u8; 32];
    bytes[0] = secret_key_seed;

    let secret_key = identity::ed25519::SecretKey::from_bytes(&mut bytes)
        .expect("this returns `Err` only if the length is wrong; the length is correct; qed");
    identity::Keypair::Ed25519(secret_key.into())
}
