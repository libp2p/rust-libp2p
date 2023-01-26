use std::env;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use either::Either;
use env_logger::{Env, Target};
use futures::{AsyncRead, AsyncWrite, StreamExt};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport::Boxed;
use libp2p::swarm::{keep_alive, NetworkBehaviour, SwarmEvent};
use libp2p::websocket::WsConfig;
use libp2p::{
    core, identity, mplex, noise, ping, webrtc, yamux, Multiaddr, PeerId, Swarm, Transport as _,
};
use redis::AsyncCommands;
use strum::EnumString;

/// Supported transports by rust-libp2p.
#[derive(Clone, Debug, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum Transport {
    Tcp,
    QuicV1,
    Webrtc,
    Ws,
}

/// Supported stream multiplexers by rust-libp2p.
#[derive(Clone, Debug, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum Muxer {
    Mplex,
    Yamux,
}

/// Supported security protocols by rust-libp2p.
#[derive(Clone, Debug, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum SecProtocol {
    Noise,
    Tls,
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
}

/// Helper function to get a ENV variable into an test parameter like `Transport`.
pub fn from_env<T>(env_var: &str) -> Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    env::var(env_var)
        .with_context(|| format!("{env_var} environment variable is not set"))?
        .parse()
        .map_err(Into::into)
}

/// Build the Tcp and Ws transports multiplexer and security protocol.
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
        Muxer::Yamux => Either::Left(yamux::YamuxConfig::default()),
        Muxer::Mplex => Either::Right(mplex::MplexConfig::default()),
    };

    let timeout = Duration::from_secs(5);

    match secure_channel_param {
        SecProtocol::Noise => builder
            .authenticate(noise::NoiseAuthenticated::xx(local_key).unwrap())
            .multiplex(mux_upgrade)
            .timeout(timeout)
            .boxed(),
        SecProtocol::Tls => builder
            .authenticate(libp2p::tls::Config::new(local_key).unwrap())
            .multiplex(mux_upgrade)
            .timeout(timeout)
            .boxed(),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    let transport_param: Transport = from_env("transport").context("unsupported transport")?;

    let ip = env::var("ip").context("ip environment variable is not set")?;

    let is_dialer = env::var("is_dialer")
        .unwrap_or_else(|_| "true".into())
        .parse::<bool>()?;

    let test_timeout = env::var("test_timeout")
        .unwrap_or_else(|_| "10".into())
        .parse::<usize>()?;

    let redis_addr = env::var("REDIS_ADDR")
        .map(|addr| format!("redis://{addr}"))
        .unwrap_or_else(|_| "redis://redis:6379".into());

    let client = redis::Client::open(redis_addr).context("Could not connect to redis")?;

    // Build the transport from the passed ENV var.
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
                from_env("security").context("unsupported secure channel")?;

            let muxer_param: Muxer = from_env("muxer").context("unsupported multiplexer")?;

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
                from_env("security").context("unsupported secure channel")?;

            let muxer_param: Muxer = from_env("muxer").context("unsupported multiplexer")?;

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

    let mut swarm = Swarm::with_tokio_executor(
        boxed_transport,
        Behaviour {
            ping: ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(1))),
            keep_alive: keep_alive::Behaviour,
        },
        local_peer_id,
    );

    let mut conn = client.get_async_connection().await?;

    log::info!("Running ping test: {}", swarm.local_peer_id());
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .target(Target::Stdout)
        .init();

    log::info!(
        "Test instance, listening for incoming connections on: {:?}.",
        local_addr
    );
    let id = swarm.listen_on(local_addr.parse()?)?;

    // Run a ping interop test. Based on `is_dialer`, either dial the address
    // retrieved via `listenAddr` key over the redis connection. Or wait to be pinged and have
    // `dialerDone` key ready on the redis connection.
    if is_dialer {
        let result: Vec<String> = conn.blpop("listenerAddr", test_timeout).await?;
        let other = result
            .get(1)
            .context("Failed to wait for listener to be ready")?;

        swarm.dial(other.parse::<Multiaddr>()?)?;
        log::info!("Test instance, dialing multiaddress on: {}.", other);

        loop {
            if let Some(SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                peer: _,
                result: Ok(ping::Success::Ping { rtt }),
            }))) = swarm.next().await
            {
                log::info!("Ping successful: {rtt:?}");
                break;
            }
        }

        conn.rpush("dialerDone", "").await?;
    } else {
        loop {
            if let Some(SwarmEvent::NewListenAddr {
                listener_id,
                address,
            }) = swarm.next().await
            {
                if address.to_string().contains("127.0.0.1") {
                    continue;
                }
                if listener_id == id {
                    let ma = format!("{address}/p2p/{local_peer_id}");
                    conn.rpush("listenerAddr", ma).await?;
                    break;
                }
            }
        }

        // Drive Swarm in the background while we await for `dialerDone` to be ready.
        tokio::spawn(async move {
            loop {
                swarm.next().await;
            }
        });

        let done: Vec<String> = conn.blpop("dialerDone", test_timeout).await?;
        done.get(1)
            .context("Failed to wait for dialer conclusion")?;
        log::info!("Ping successful");
    }

    Ok(())
}
