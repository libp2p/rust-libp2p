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

use anyhow::{Context, Result};
use clap::Parser;
use futures::{future::Either, stream::StreamExt};
use libp2p::{
    core::{
        multiaddr::{Multiaddr, Protocol},
        muxing::StreamMuxerBox,
        transport::Transport,
        upgrade,
    },
    dcutr, identify, identity, noise, ping, quic, relay,
    swarm::{NetworkBehaviour, SwarmBuilder, SwarmEvent},
    tcp, yamux, PeerId, Swarm,
};
use log::{info, LevelFilter};
use redis::AsyncCommands;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::str::FromStr;

#[derive(Debug, Parser)]
#[clap(name = "libp2p DCUtR client")]
struct Opts {
    /// The mode (listening or dialing).
    #[clap(long)]
    mode: Mode,

    /// The transport (tcp or quic).
    #[clap(long)]
    transport: TransportProtocol,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(LevelFilter::Info)
        .parse_default_env()
        .init();

    let opts = Opts::parse();

    let client = redis::Client::open("redis://redis:6379")?;
    let mut connection = client.get_async_connection().await?;

    let redis_key = match opts.transport {
        TransportProtocol::Tcp => "RELAY_TCP_ADDRESS",
        TransportProtocol::Quic => "RELAY_QUIC_ADDRESS",
    };

    let relay_address = connection
        .blpop::<_, HashMap<String, String>>(redis_key, 10)
        .await?
        .remove(redis_key)
        .expect("key that we asked for to be present")
        .parse::<Multiaddr>()?;

    let mut swarm = make_swarm()?;

    // Both parties must have a listener for the hole-punch to work.
    let listen_addr = match opts.transport {
        TransportProtocol::Tcp => Multiaddr::empty()
            .with(Protocol::Ip4(Ipv4Addr::UNSPECIFIED))
            .with(Protocol::Tcp(0)),
        TransportProtocol::Quic => Multiaddr::empty()
            .with(Protocol::Ip4(Ipv4Addr::UNSPECIFIED))
            .with(Protocol::Udp(0))
            .with(Protocol::QuicV1),
    };
    let expected_listener_id = swarm
        .listen_on(listen_addr)
        .context("Failed to listen on address")?;
    let mut listen_addresses = 0;

    // We should have at least two listen addresses, one for localhost and the actual interface.
    while listen_addresses < 2 {
        if let SwarmEvent::NewListenAddr {
            listener_id,
            address,
        } = swarm.next().await.unwrap()
        {
            if listener_id == expected_listener_id {
                listen_addresses += 1;
            }

            info!("Listening on {address}");
        }
    }

    swarm.dial(relay_address.clone())?;

    // Connect to the relay server. Not for the reservation or relayed connection, but to learn our local public address.
    // FIXME: This should not be necessary. Perhaps dcutr should also already consider external address _candidates_?
    loop {
        if let SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
            info: identify::Info { observed_addr, .. },
            ..
        })) = swarm.next().await.unwrap()
        {
            info!("Relay told us our public address: {:?}", observed_addr);
            swarm.add_external_address(observed_addr);
            break;
        }
    }

    info!("Connected to the relay");

    match opts.mode {
        Mode::Dial => {
            let remote_peer_id = connection
                .blpop::<_, HashMap<String, String>>("LISTEN_CLIENT_PEER_ID", 10)
                .await?
                .remove("LISTEN_CLIENT_PEER_ID")
                .expect("key that we asked for to be present")
                .parse()?;

            swarm.dial(
                relay_address
                    .with(Protocol::P2pCircuit)
                    .with(Protocol::P2p(remote_peer_id)),
            )?;
        }
        Mode::Listen => {
            swarm.listen_on(relay_address.with(Protocol::P2pCircuit))?;
        }
    }

    loop {
        match swarm.next().await.unwrap() {
            SwarmEvent::Behaviour(BehaviourEvent::RelayClient(
                relay::client::Event::ReservationReqAccepted { .. },
            )) => {
                assert!(opts.mode == Mode::Listen);
                info!("Relay accepted our reservation request.");

                connection
                    .rpush("LISTEN_CLIENT_PEER_ID", swarm.local_peer_id().to_string())
                    .await?;
            }
            SwarmEvent::Behaviour(BehaviourEvent::RelayClient(event)) => {
                info!("{:?}", event)
            }
            SwarmEvent::Behaviour(BehaviourEvent::Dcutr(
                dcutr::Event::DirectConnectionUpgradeSucceeded { remote_peer_id },
            )) => {
                info!("Successfully hole-punched to {remote_peer_id}");
                return Ok(());
            }
            SwarmEvent::Behaviour(BehaviourEvent::Identify(_)) => {}
            SwarmEvent::Behaviour(BehaviourEvent::Ping(_)) => {}
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                info!("Connected to {peer_id}");
            }
            SwarmEvent::OutgoingConnectionError { error, .. } => {
                anyhow::bail!(error)
            }
            _ => {}
        }
    }
}

fn make_swarm() -> Result<Swarm<Behaviour>> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    info!("Local peer id: {local_peer_id}");

    let (relay_transport, client) = relay::client::new(local_peer_id);

    let transport = {
        let relay_tcp_quic_transport = relay_transport
            .or_transport(tcp::tokio::Transport::new(
                tcp::Config::default().port_reuse(true),
            ))
            .upgrade(upgrade::Version::V1)
            .authenticate(noise::Config::new(&local_key)?)
            .multiplex(yamux::Config::default())
            .or_transport(quic::tokio::Transport::new(quic::Config::new(&local_key)));

        relay_tcp_quic_transport
            .map(|either_output, _| match either_output {
                Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
                Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            })
            .boxed()
    };

    let behaviour = Behaviour {
        relay_client: client,
        ping: ping::Behaviour::new(ping::Config::new()),
        identify: identify::Behaviour::new(identify::Config::new(
            "/TODO/0.0.1".to_string(),
            local_key.public(),
        )),
        dcutr: dcutr::Behaviour::new(local_peer_id),
    };

    Ok(SwarmBuilder::with_tokio_executor(transport, behaviour, local_peer_id).build())
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

#[derive(Clone, Debug, PartialEq, Parser)]
enum TransportProtocol {
    Tcp,
    Quic,
}

impl FromStr for TransportProtocol {
    type Err = String;
    fn from_str(mode: &str) -> Result<Self, Self::Err> {
        match mode {
            "tcp" => Ok(TransportProtocol::Tcp),
            "quic" => Ok(TransportProtocol::Quic),
            _ => Err("Expected either 'tcp' or 'quic'".to_string()),
        }
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay_client: relay::client::Behaviour,
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    dcutr: dcutr::Behaviour,
}
