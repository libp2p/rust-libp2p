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
use futures::{future::Either, stream::StreamExt};
use libp2p::{
    core::{
        multiaddr::{Multiaddr, Protocol},
        muxing::StreamMuxerBox,
        transport::Transport,
        upgrade,
    },
    dcutr, identify, identity, noise, quic, relay,
    swarm::{NetworkBehaviour, SwarmBuilder, SwarmEvent},
    tcp, yamux, PeerId, Swarm,
};
use redis::AsyncCommands;
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;

/// The redis key we push the relay's TCP listen address to.
const RELAY_TCP_ADDRESS: &str = "RELAY_TCP_ADDRESS";
/// The redis key we push the relay's QUIC listen address to.
const RELAY_QUIC_ADDRESS: &str = "RELAY_QUIC_ADDRESS";
/// The redis key we push the listen client's PeerId to.
const LISTEN_CLIENT_PEER_ID: &str = "LISTEN_CLIENT_PEER_ID";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .parse_default_env()
        .init();

    let mode = get_env::<Mode>("MODE")?;
    let transport = get_env::<TransportProtocol>("TRANSPORT")?;

    let mut redis = RedisClient::new("redis", 6379).await?;

    match mode {
        Mode::Dial => {
            let relay_addr = match transport {
                TransportProtocol::Tcp => redis.pop::<Multiaddr>(RELAY_TCP_ADDRESS).await?,
                TransportProtocol::Quic => redis.pop::<Multiaddr>(RELAY_QUIC_ADDRESS).await?,
            };

            let mut swarm = make_swarm()?;
            client_listen_on_transport(&mut swarm, transport).await?;
            client_connect_to_relay(&mut swarm, relay_addr.clone()).await?;

            let remote_peer_id = redis.pop(LISTEN_CLIENT_PEER_ID).await?;

            swarm.dial(
                relay_addr
                    .with(Protocol::P2pCircuit)
                    .with(Protocol::P2p(remote_peer_id)),
            )?;

            loop {
                match swarm.next().await.unwrap() {
                    SwarmEvent::Behaviour(BehaviourEvent::Dcutr(
                        dcutr::Event::DirectConnectionUpgradeSucceeded { remote_peer_id },
                    )) => {
                        log::info!("Successfully hole-punched to {remote_peer_id}");
                        return Ok(());
                    }
                    SwarmEvent::Behaviour(BehaviourEvent::Dcutr(
                        dcutr::Event::DirectConnectionUpgradeFailed {
                            remote_peer_id,
                            error,
                        },
                    )) => {
                        log::info!("Failed to hole-punched to {remote_peer_id}");
                        return Err(anyhow::Error::new(error));
                    }
                    SwarmEvent::OutgoingConnectionError { error, .. } => {
                        anyhow::bail!(error)
                    }
                    _ => {}
                }
            }
        }
        Mode::Listen => {
            let relay_addr = match transport {
                TransportProtocol::Tcp => redis.pop::<Multiaddr>(RELAY_TCP_ADDRESS).await?,
                TransportProtocol::Quic => redis.pop::<Multiaddr>(RELAY_QUIC_ADDRESS).await?,
            };

            let mut swarm = make_swarm()?;
            client_listen_on_transport(&mut swarm, transport).await?;
            client_connect_to_relay(&mut swarm, relay_addr.clone()).await?;

            swarm.listen_on(relay_addr.with(Protocol::P2pCircuit))?;

            loop {
                match swarm.next().await.unwrap() {
                    SwarmEvent::Behaviour(BehaviourEvent::RelayClient(
                        relay::client::Event::ReservationReqAccepted { .. },
                    )) => {
                        log::info!("Relay accepted our reservation request.");

                        redis
                            .push(LISTEN_CLIENT_PEER_ID, swarm.local_peer_id())
                            .await?;
                    }
                    SwarmEvent::Behaviour(BehaviourEvent::Dcutr(
                        dcutr::Event::DirectConnectionUpgradeSucceeded { remote_peer_id },
                    )) => {
                        log::info!("Successfully hole-punched to {remote_peer_id}");
                        return Ok(());
                    }
                    SwarmEvent::Behaviour(BehaviourEvent::Dcutr(
                        dcutr::Event::DirectConnectionUpgradeFailed {
                            remote_peer_id,
                            error,
                        },
                    )) => {
                        log::info!("Failed to hole-punched to {remote_peer_id}");
                        return Err(anyhow::Error::new(error));
                    }
                    SwarmEvent::OutgoingConnectionError { error, .. } => {
                        anyhow::bail!(error)
                    }
                    _ => {}
                }
            }
        }
    }
}

fn get_env<T>(key: &'static str) -> Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let val = std::env::var(key)
        .with_context(|| format!("Missing env var `{key}`"))?
        .parse()
        .with_context(|| format!("Failed to parse `{key}`)"))?;

    Ok(val)
}

async fn client_connect_to_relay(
    swarm: &mut Swarm<Behaviour>,
    relay_addr: Multiaddr,
) -> Result<()> {
    // Connect to the relay server.
    swarm.dial(relay_addr.clone())?;

    loop {
        if let SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {
            info: identify::Info { observed_addr, .. },
            ..
        })) = swarm.next().await.unwrap()
        {
            log::info!("Relay told us our public address: {observed_addr}");
            swarm.add_external_address(observed_addr);
            break;
        }
    }

    log::info!("Connected to the relay");
    Ok(())
}

async fn client_listen_on_transport(
    swarm: &mut Swarm<Behaviour>,
    transport: TransportProtocol,
) -> Result<()> {
    let listen_addr = match transport {
        TransportProtocol::Tcp => tcp_addr(Ipv4Addr::UNSPECIFIED.into()),
        TransportProtocol::Quic => quic_addr(Ipv4Addr::UNSPECIFIED.into()),
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

            log::info!("Listening on {address}");
        }
    }
    Ok(())
}

fn tcp_addr(addr: IpAddr) -> Multiaddr {
    Multiaddr::empty().with(addr.into()).with(Protocol::Tcp(0))
}

fn quic_addr(addr: IpAddr) -> Multiaddr {
    Multiaddr::empty()
        .with(addr.into())
        .with(Protocol::Udp(0))
        .with(Protocol::QuicV1)
}

fn make_swarm() -> Result<Swarm<Behaviour>> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    log::info!("Local peer id: {local_peer_id}");

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
        identify: identify::Behaviour::new(identify::Config::new(
            "/hole-punch-tests/1".to_owned(),
            local_key.public(),
        )),
        dcutr: dcutr::Behaviour::new(local_peer_id),
    };

    Ok(SwarmBuilder::with_tokio_executor(transport, behaviour, local_peer_id).build())
}

struct RedisClient {
    inner: redis::aio::Connection,
}

impl RedisClient {
    async fn new(host: &str, port: u16) -> Result<Self> {
        let client = redis::Client::open(format!("redis://{host}:{port}/"))
            .context("Bad redis server URL")?;
        let connection = client
            .get_async_connection()
            .await
            .context("Failed to connect to redis server")?;

        Ok(Self { inner: connection })
    }

    async fn push(&mut self, key: &str, value: impl ToString) -> Result<()> {
        self.inner.rpush(key, value.to_string()).await?;

        Ok(())
    }

    async fn pop<V>(&mut self, key: &str) -> Result<V>
    where
        V: FromStr,
        V::Err: std::error::Error + Send + Sync + 'static,
    {
        let value = self
            .inner
            .blpop::<_, HashMap<String, String>>(key, 10)
            .await?
            .remove(key)
            .expect("key that we asked for to be present")
            .parse()?;

        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TransportProtocol {
    Tcp,
    Quic,
}

impl FromStr for TransportProtocol {
    type Err = io::Error;
    fn from_str(mode: &str) -> Result<Self, Self::Err> {
        match mode {
            "tcp" => Ok(TransportProtocol::Tcp),
            "quic" => Ok(TransportProtocol::Quic),
            _ => Err(io::Error::new(
                io::ErrorKind::Other,
                "Expected either 'tcp' or 'quic'",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Dial,
    Listen,
}

impl FromStr for Mode {
    type Err = io::Error;
    fn from_str(mode: &str) -> Result<Self, Self::Err> {
        match mode {
            "dial" => Ok(Mode::Dial),
            "listen" => Ok(Mode::Listen),
            _ => Err(io::Error::new(
                io::ErrorKind::Other,
                "Expected either 'dial' or 'listen'",
            )),
        }
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    relay_client: relay::client::Behaviour,
    identify: identify::Behaviour,
    dcutr: dcutr::Behaviour,
}
