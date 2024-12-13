// Copyright 2023 Protocol Labs.
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

use std::{
    collections::HashMap,
    fmt, io,
    net::{IpAddr, Ipv4Addr},
    str::FromStr,
    time::Duration,
};

use anyhow::{Context, Result};
use either::Either;
use futures::stream::StreamExt;
use libp2p::{
    core::{
        multiaddr::{Multiaddr, Protocol},
        transport::ListenerId,
    },
    dcutr, identify, noise, ping, relay,
    swarm::{dial_opts::DialOpts, ConnectionId, NetworkBehaviour, SwarmEvent},
    tcp, yamux, Swarm,
};
use redis::AsyncCommands;

/// The redis key we push the relay's TCP listen address to.
const RELAY_TCP_ADDRESS: &str = "RELAY_TCP_ADDRESS";
/// The redis key we push the relay's QUIC listen address to.
const RELAY_QUIC_ADDRESS: &str = "RELAY_QUIC_ADDRESS";
/// The redis key we push the listen client's PeerId to.
const LISTEN_CLIENT_PEER_ID: &str = "LISTEN_CLIENT_PEER_ID";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .parse_filters("debug,netlink_proto=warn,rustls=warn,multistream_select=warn,libp2p_core::transport::choice=off,libp2p_swarm::connection=warn,libp2p_quic=trace")
        .parse_default_env()
        .init();

    let mode = get_env("MODE")?;
    let transport = get_env("TRANSPORT")?;

    let mut redis = RedisClient::new("redis", 6379).await?;

    let relay_addr = match transport {
        TransportProtocol::Tcp => redis.pop::<Multiaddr>(RELAY_TCP_ADDRESS).await?,
        TransportProtocol::Quic => redis.pop::<Multiaddr>(RELAY_QUIC_ADDRESS).await?,
    };

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::new().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_quic()
        .with_relay_client(noise::Config::new, yamux::Config::default)?
        .with_behaviour(|key, relay_client| {
            Ok(Behaviour {
                relay_client,
                identify: identify::Behaviour::new(identify::Config::new(
                    "/hole-punch-tests/1".to_owned(),
                    key.public(),
                )),
                dcutr: dcutr::Behaviour::new(key.public().to_peer_id()),
                ping: ping::Behaviour::new(
                    ping::Config::default().with_interval(Duration::from_secs(1)),
                ),
            })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    client_listen_on_transport(&mut swarm, transport).await?;
    let id = client_setup(&mut swarm, &mut redis, relay_addr.clone(), mode).await?;

    let mut hole_punched_peer_connection = None;

    loop {
        match (
            swarm.next().await.unwrap(),
            hole_punched_peer_connection,
            id,
        ) {
            (
                SwarmEvent::Behaviour(BehaviourEvent::RelayClient(
                    relay::client::Event::ReservationReqAccepted { .. },
                )),
                _,
                _,
            ) => {
                tracing::info!("Relay accepted our reservation request.");

                redis
                    .push(LISTEN_CLIENT_PEER_ID, swarm.local_peer_id())
                    .await?;
            }
            (
                SwarmEvent::Behaviour(BehaviourEvent::Dcutr(dcutr::Event {
                    remote_peer_id,
                    result: Ok(connection_id),
                })),
                _,
                _,
            ) => {
                tracing::info!("Successfully hole-punched to {remote_peer_id}");

                hole_punched_peer_connection = Some(connection_id);
            }
            (
                SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                    connection,
                    result: Ok(rtt),
                    ..
                })),
                Some(hole_punched_connection),
                _,
            ) if mode == Mode::Dial && connection == hole_punched_connection => {
                println!("{}", serde_json::to_string(&Report::new(rtt))?);

                return Ok(());
            }
            (
                SwarmEvent::Behaviour(BehaviourEvent::Dcutr(dcutr::Event {
                    remote_peer_id,
                    result: Err(error),
                    ..
                })),
                _,
                _,
            ) => {
                tracing::info!("Failed to hole-punched to {remote_peer_id}");
                return Err(anyhow::Error::new(error));
            }
            (
                SwarmEvent::ListenerClosed {
                    listener_id,
                    reason: Err(e),
                    ..
                },
                _,
                Either::Left(reservation),
            ) if listener_id == reservation => {
                anyhow::bail!("Reservation on relay failed: {e}");
            }
            (
                SwarmEvent::OutgoingConnectionError {
                    connection_id,
                    error,
                    ..
                },
                _,
                Either::Right(circuit),
            ) if connection_id == circuit => {
                anyhow::bail!("Circuit request relay failed: {error}");
            }
            _ => {}
        }
    }
}

#[derive(serde::Serialize)]
struct Report {
    rtt_to_holepunched_peer_millis: u128,
}

impl Report {
    fn new(rtt: Duration) -> Self {
        Self {
            rtt_to_holepunched_peer_millis: rtt.as_millis(),
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

            tracing::info!("Listening on {address}");
        }
    }
    Ok(())
}

async fn client_setup(
    swarm: &mut Swarm<Behaviour>,
    redis: &mut RedisClient,
    relay_addr: Multiaddr,
    mode: Mode,
) -> Result<Either<ListenerId, ConnectionId>> {
    let either = match mode {
        Mode::Listen => {
            let id = swarm.listen_on(relay_addr.with(Protocol::P2pCircuit))?;

            Either::Left(id)
        }
        Mode::Dial => {
            let remote_peer_id = redis.pop(LISTEN_CLIENT_PEER_ID).await?;

            let opts = DialOpts::from(
                relay_addr
                    .with(Protocol::P2pCircuit)
                    .with(Protocol::P2p(remote_peer_id)),
            );
            let id = opts.connection_id();

            swarm.dial(opts)?;

            Either::Right(id)
        }
    };

    Ok(either)
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
        let value = value.to_string();

        tracing::debug!("Pushing {key}={value} to redis");

        self.inner.rpush(key, value).await.map_err(Into::into)
    }

    async fn pop<V>(&mut self, key: &str) -> Result<V>
    where
        V: FromStr + fmt::Display,
        V::Err: std::error::Error + Send + Sync + 'static,
    {
        tracing::debug!("Fetching {key} from redis");

        let value = self
            .inner
            .blpop::<_, HashMap<String, String>>(key, 0.0)
            .await?
            .remove(key)
            .with_context(|| format!("Failed to get value for {key} from redis"))?
            .parse()?;

        tracing::debug!("{key}={value}");

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
    ping: ping::Behaviour,
}
