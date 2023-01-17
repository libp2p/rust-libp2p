use std::collections::HashSet;
use std::env;
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite, StreamExt};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::core::upgrade::EitherUpgrade;
use libp2p::swarm::{keep_alive, NetworkBehaviour, SwarmEvent};
use libp2p::websocket::WsConfig;
use libp2p::{
    core, identity, mplex, noise, ping, webrtc, yamux, Multiaddr, PeerId, Swarm, Transport as _,
};
use testplan::{run_ping, Muxer, PingSwarm, SecProtocol, Transport};

fn build_builder<T, C>(
    builder: core::transport::upgrade::Builder<T>,
    secure_channel_param: SecProtocol,
    muxer_param: Muxer,
    local_key: &identity::Keypair,
) -> Boxed<(libp2p::PeerId, StreamMuxerBox)>
where
    T: libp2p::Transport<Output = C> + Send + Unpin + 'static,
    <T as libp2p::Transport>::Error: Sync + Send + 'static,
    <T as libp2p::Transport>::ListenerUpgrade: Send,
    <T as libp2p::Transport>::Dial: Send,
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let mux_upgrade = match muxer_param {
        Muxer::Yamux => EitherUpgrade::A(yamux::YamuxConfig::default()),
        Muxer::Mplex => EitherUpgrade::B(mplex::MplexConfig::default()),
    };

    let timeout = Duration::from_secs(5);

    match secure_channel_param {
        SecProtocol::Noise => builder
            .authenticate(noise::NoiseAuthenticated::xx(&local_key).unwrap())
            .multiplex(mux_upgrade)
            .timeout(timeout)
            .boxed(),
        SecProtocol::Tls => builder
            .authenticate(libp2p::tls::Config::new(&local_key).unwrap())
            .multiplex(mux_upgrade)
            .timeout(timeout)
            .boxed(),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    let transport_param: Transport =
        testplan::from_env("transport").context("unsupported transport")?;

    let ip = env::var("ip").context("ip environment variable is not set")?;

    let is_dialer = env::var("is_dialer")
        .unwrap_or("true".into())
        .parse::<bool>()?;

    let redis_addr = env::var("REDIS_ADDR")
        .map(|addr| format!("redis://{addr}"))
        .unwrap_or("redis://redis:6379".into());

    let client = redis::Client::open(redis_addr).context("Could not connect to redis")?;

    let (boxed_transport, local_addr) = match transport_param {
        Transport::QuicV1 => {
            let builder =
                libp2p::quic::tokio::Transport::new(libp2p::quic::Config::new(&local_key))
                    .map(|(p, c), _| (p, StreamMuxerBox::new(c)));
            (builder.boxed(), format!("/ip4/{ip}/udp/0/quic-v1"))
        }
        Transport::Tcp => {
            let builder = libp2p::tcp::tokio::Transport::new(libp2p::tcp::Config::new())
                .upgrade(libp2p::core::upgrade::Version::V1Lazy);

            let secure_channel_param: SecProtocol =
                testplan::from_env("security").context("unsupported secure channel")?;

            let muxer_param: Muxer =
                testplan::from_env("muxer").context("unsupported multiplexer")?;

            (
                build_builder(builder, secure_channel_param, muxer_param, &local_key),
                format!("/ip4/{ip}/tcp/0"),
            )
        }
        Transport::Ws => {
            let builder = WsConfig::new(libp2p::tcp::tokio::Transport::new(
                libp2p::tcp::Config::new(),
            ))
            .upgrade(libp2p::core::upgrade::Version::V1Lazy);

            let secure_channel_param: SecProtocol =
                testplan::from_env("security").context("unsupported secure channel")?;

            let muxer_param: Muxer =
                testplan::from_env("muxer").context("unsupported multiplexer")?;

            (
                build_builder(builder, secure_channel_param, muxer_param, &local_key),
                format!("/ip4/{ip}/tcp/0/ws"),
            )
        }
        Transport::Webrtc => (
            webrtc::tokio::Transport::new(
                local_key,
                webrtc::tokio::Certificate::generate(&mut rand::thread_rng())?,
            )
            .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn)))
            .boxed(),
            format!("/ip4/{ip}/udp/0/webrtc"),
        ),
    };

    let swarm = OrphanRuleWorkaround(Swarm::with_tokio_executor(
        boxed_transport,
        Behaviour {
            ping: ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(1))),
            keep_alive: keep_alive::Behaviour,
        },
        local_peer_id,
    ));

    // Use peer id as a String so that `run_ping` does not depend on a specific libp2p version.
    let local_peer_id = local_peer_id.to_string();
    run_ping(client, swarm, &local_addr, &local_peer_id, is_dialer).await?;

    Ok(())
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
}
struct OrphanRuleWorkaround(Swarm<Behaviour>);

#[async_trait]
impl PingSwarm for OrphanRuleWorkaround {
    async fn listen_on(&mut self, address: &str) -> Result<String> {
        let id = self.0.listen_on(address.parse()?)?;

        loop {
            if let Some(SwarmEvent::NewListenAddr {
                listener_id,
                address,
            }) = self.0.next().await
            {
                if address.to_string().contains("127.0.0.1") {
                    continue;
                }
                if listener_id == id {
                    return Ok(address.to_string());
                }
            }
        }
    }

    fn dial(&mut self, address: &str) -> Result<()> {
        self.0.dial(address.parse::<Multiaddr>()?)?;

        Ok(())
    }

    async fn await_connections(&mut self, number: usize) {
        let mut connected = HashSet::with_capacity(number);

        while connected.len() < number {
            if let Some(SwarmEvent::ConnectionEstablished { peer_id, .. }) = self.0.next().await {
                connected.insert(peer_id);
            }
        }
    }

    async fn await_pings(&mut self, number: usize) -> Vec<Duration> {
        let mut received_pings = Vec::with_capacity(number);

        while received_pings.len() < number {
            if let Some(SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                peer: _,
                result: Ok(ping::Success::Ping { rtt }),
            }))) = self.0.next().await
            {
                received_pings.push(rtt);
            }
        }

        received_pings
    }

    async fn loop_on_next(&mut self) {
        loop {
            self.0.next().await;
        }
    }

    fn local_peer_id(&self) -> String {
        self.0.local_peer_id().to_string()
    }
}
