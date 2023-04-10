use std::env;
use std::str::FromStr;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use either::Either;
use env_logger::{Env, Target};
use futures::{future, AsyncRead, AsyncWrite, StreamExt};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::upgrade::{MapInboundUpgrade, MapOutboundUpgrade, Version};
use libp2p::noise::{NoiseOutput, X25519Spec, XX};
use libp2p::swarm::{keep_alive, NetworkBehaviour, SwarmEvent};
use libp2p::tls::TlsStream;
use libp2p::websocket::WsConfig;
use libp2p::{
    identity, noise, ping, swarm::SwarmBuilder, tcp, tls, yamux, InboundUpgradeExt, Multiaddr,
    OutboundUpgradeExt, PeerId, Transport as _,
};
use libp2p_mplex as mplex;
use libp2p_quic as quic;
use libp2p_webrtc as webrtc;
use redis::AsyncCommands;

#[tokio::main]
async fn main() -> Result<()> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    let transport_param: Transport = from_env("transport")?;

    let ip = env::var("ip").context("ip environment variable is not set")?;

    let is_dialer = env::var("is_dialer")
        .unwrap_or_else(|_| "true".into())
        .parse::<bool>()?;

    let test_timeout = env::var("test_timeout_seconds")
        .unwrap_or_else(|_| "180".into())
        .parse::<u64>()?;

    let redis_addr = env::var("redis_addr")
        .map(|addr| format!("redis://{addr}"))
        .unwrap_or_else(|_| "redis://redis:6379".into());

    let client = redis::Client::open(redis_addr).context("Could not connect to redis")?;

    // Build the transport from the passed ENV var.
    let (boxed_transport, local_addr) = match transport_param {
        Transport::QuicV1 => (
            quic::tokio::Transport::new(quic::Config::new(&local_key))
                .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
                .boxed(),
            format!("/ip4/{ip}/udp/0/quic-v1"),
        ),
        Transport::Tcp => (
            tcp::tokio::Transport::new(tcp::Config::new())
                .upgrade(Version::V1Lazy)
                .authenticate(secure_channel_protocol_from_env(&local_key)?)
                .multiplex(muxer_protocol_from_env()?)
                .timeout(Duration::from_secs(5))
                .boxed(),
            format!("/ip4/{ip}/tcp/0"),
        ),
        Transport::Ws => (
            WsConfig::new(tcp::tokio::Transport::new(tcp::Config::new()))
                .upgrade(Version::V1Lazy)
                .authenticate(secure_channel_protocol_from_env(&local_key)?)
                .multiplex(muxer_protocol_from_env()?)
                .timeout(Duration::from_secs(5))
                .boxed(),
            format!("/ip4/{ip}/tcp/0/ws"),
        ),
        Transport::Webrtc => (
            webrtc::tokio::Transport::new(
                local_key,
                webrtc::tokio::Certificate::generate(&mut rand::thread_rng())?,
            )
            .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn)))
            .boxed(),
            format!("/ip4/{ip}/udp/0/webrtc-direct"),
        ),
    };

    let mut swarm = SwarmBuilder::with_tokio_executor(
        boxed_transport,
        Behaviour {
            ping: ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(1))),
            keep_alive: keep_alive::Behaviour,
        },
        local_peer_id,
    )
    .build();

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
        let result: Vec<String> = conn.blpop("listenerAddr", test_timeout as usize).await?;
        let other = result
            .get(1)
            .context("Failed to wait for listener to be ready")?;

        let handshake_start = Instant::now();

        swarm.dial(other.parse::<Multiaddr>()?)?;
        log::info!("Test instance, dialing multiaddress on: {}.", other);

        let rtt = loop {
            if let Some(SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                peer: _,
                result: Ok(ping::Success::Ping { rtt }),
            }))) = swarm.next().await
            {
                log::info!("Ping successful: {rtt:?}");
                break rtt.as_millis() as f32;
            }
        };

        let handshake_plus_ping = handshake_start.elapsed().as_millis() as f32;
        println!(
            r#"{{"handshakePlusOneRTTMillis": {handshake_plus_ping:.1}, "pingRTTMilllis": {rtt:.1}}}"#
        );
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
        tokio::time::sleep(Duration::from_secs(test_timeout)).await;
        bail!("Test should have been killed by the test runner!");
    }

    Ok(())
}

fn secure_channel_protocol_from_env<C: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    identity: &identity::Keypair,
) -> Result<
    MapOutboundUpgrade<
        MapInboundUpgrade<
            Either<noise::NoiseAuthenticated<XX, X25519Spec, ()>, tls::Config>,
            MapSecOutputFn<C>,
        >,
        MapSecOutputFn<C>,
    >,
> {
    let either_sec_upgrade = match from_env("security")? {
        SecProtocol::Noise => Either::Left(
            noise::NoiseAuthenticated::xx(identity).context("failed to intialise noise")?,
        ),
        SecProtocol::Tls => {
            Either::Right(tls::Config::new(identity).context("failed to initialise tls")?)
        }
    };

    Ok(either_sec_upgrade
        .map_inbound(factor_peer_id as MapSecOutputFn<C>)
        .map_outbound(factor_peer_id as MapSecOutputFn<C>))
}

type SecOutput<C> = future::Either<(PeerId, NoiseOutput<C>), (PeerId, TlsStream<C>)>;
type MapSecOutputFn<C> = fn(SecOutput<C>) -> (PeerId, future::Either<NoiseOutput<C>, TlsStream<C>>);

fn factor_peer_id<C>(
    output: SecOutput<C>,
) -> (PeerId, future::Either<NoiseOutput<C>, TlsStream<C>>) {
    match output {
        future::Either::Left((peer, stream)) => (peer, future::Either::Left(stream)),
        future::Either::Right((peer, stream)) => (peer, future::Either::Right(stream)),
    }
}

fn muxer_protocol_from_env() -> Result<Either<yamux::YamuxConfig, mplex::MplexConfig>> {
    Ok(match from_env("muxer")? {
        Muxer::Yamux => Either::Left(yamux::YamuxConfig::default()),
        Muxer::Mplex => Either::Right(mplex::MplexConfig::new()),
    })
}

/// Supported transports by rust-libp2p.
#[derive(Clone, Debug)]
pub enum Transport {
    Tcp,
    QuicV1,
    Webrtc,
    Ws,
}

impl FromStr for Transport {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "tcp" => Self::Tcp,
            "quic-v1" => Self::QuicV1,
            "webrtc" => Self::Webrtc,
            "ws" => Self::Ws,
            other => bail!("unknown transport {other}"),
        })
    }
}

/// Supported stream multiplexers by rust-libp2p.
#[derive(Clone, Debug)]
pub enum Muxer {
    Mplex,
    Yamux,
}

impl FromStr for Muxer {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "mplex" => Self::Mplex,
            "yamux" => Self::Yamux,
            other => bail!("unknown muxer {other}"),
        })
    }
}

/// Supported security protocols by rust-libp2p.
#[derive(Clone, Debug)]
pub enum SecProtocol {
    Noise,
    Tls,
}

impl FromStr for SecProtocol {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "noise" => Self::Noise,
            "tls" => Self::Tls,
            other => bail!("unknown security protocol {other}"),
        })
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
}

/// Helper function to get a ENV variable into an test parameter like `Transport`.
pub fn from_env<T>(env_var: &str) -> Result<T>
where
    T: FromStr<Err = anyhow::Error>,
{
    env::var(env_var)
        .with_context(|| format!("{env_var} environment variable is not set"))?
        .parse()
        .map_err(Into::into)
}
