use std::env;
use std::str::FromStr;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use env_logger::{Env, Target};
use futures::StreamExt;
use redis::AsyncCommands;
use strum::EnumString;

use libp2p::core::upgrade::{EitherUpgrade, Version};
use libp2p::noise::{X25519Spec, XX};
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::websocket::WsConfig;
use libp2p::{identity, mplex, noise, ping, tcp, yamux, Multiaddr, PeerId, Transport as _};

#[tokio::main]
async fn main() -> Result<()> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    let transport_param: Transport = from_env("transport").context("unsupported transport")?;

    let ip = env::var("ip").context("ip environment variable is not set")?;

    let is_dialer = env::var("is_dialer")
        .unwrap_or_else(|_| "true".into())
        .parse::<bool>()?;

    let test_timeout = env::var("test_timeout_seconds")
        .unwrap_or_else(|_| "10".into())
        .parse::<u64>()?;

    let redis_addr = env::var("redis_addr")
        .map(|addr| format!("redis://{addr}"))
        .unwrap_or_else(|_| "redis://redis:6379".into());

    let client = redis::Client::open(redis_addr).context("Could not connect to redis")?;

    // Build the transport from the passed ENV var.
    let (boxed_transport, local_addr) = match transport_param {
        Transport::Tcp => (
            tcp::TokioTcpTransport::new(tcp::GenTcpConfig::new())
                .upgrade(Version::V1Lazy)
                .authenticate(secure_channel_protocol_from_env(&local_key)?)
                .multiplex(muxer_protocol_from_env()?)
                .timeout(Duration::from_secs(5))
                .boxed(),
            format!("/ip4/{ip}/tcp/0"),
        ),
        Transport::Ws => (
            WsConfig::new(tcp::TokioTcpTransport::new(tcp::GenTcpConfig::new()))
                .upgrade(Version::V1Lazy)
                .authenticate(secure_channel_protocol_from_env(&local_key)?)
                .multiplex(muxer_protocol_from_env()?)
                .timeout(Duration::from_secs(5))
                .boxed(),
            format!("/ip4/{ip}/tcp/0/ws"),
        ),
    };

    let mut swarm = SwarmBuilder::new(
        boxed_transport,
        ping::Behaviour::new(
            ping::Config::new()
                .with_keep_alive(true)
                .with_interval(Duration::from_secs(1)),
        ),
        local_peer_id,
    )
    .executor(Box::new(|f| {
        tokio::spawn(f);
    }))
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
            if let Some(SwarmEvent::Behaviour(ping::Event {
                peer: _,
                result: Ok(ping::Success::Ping { rtt }),
            })) = swarm.next().await
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

fn secure_channel_protocol_from_env(
    identity: &identity::Keypair,
) -> Result<noise::NoiseAuthenticated<XX, X25519Spec, ()>> {
    Ok(
        match from_env("security").context("unsupported secure channel")? {
            SecProtocol::Noise => noise::NoiseConfig::xx(
                noise::Keypair::<X25519Spec>::new()
                    .into_authentic(identity)
                    .context("failed to intialise noise")?,
            )
            .into_authenticated(),
        },
    )
}

fn muxer_protocol_from_env() -> Result<EitherUpgrade<yamux::YamuxConfig, mplex::MplexConfig>> {
    Ok(
        match from_env("muxer").context("unsupported multiplexer")? {
            Muxer::Yamux => EitherUpgrade::A(yamux::YamuxConfig::default()),
            Muxer::Mplex => EitherUpgrade::B(mplex::MplexConfig::new()),
        },
    )
}

/// Supported transports by rust-libp2p.
#[derive(Clone, Debug, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum Transport {
    Tcp,
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
