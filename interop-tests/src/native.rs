use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use either::Either;
use env_logger::{Env, Target};
use futures::StreamExt;
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::upgrade::Version;
use libp2p::swarm::{keep_alive, SwarmBuilder, SwarmEvent};
use libp2p::websocket::WsConfig;
use libp2p::{identity, noise, ping, tcp, tls, yamux, Multiaddr, PeerId, Transport as _};
use libp2p_mplex as mplex;
use libp2p_quic as quic;
use libp2p_webrtc as webrtc;
use redis::AsyncCommands;

use crate::{from_env, Behaviour, BehaviourEvent, Muxer, SecProtocol, Transport};

pub async fn run_test(
    transport: Transport,
    ip: &str,
    is_dialer: bool,
    test_timeout: Duration,
    redis_addr: &str,
) -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .target(Target::Stdout)
        .init();

    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    let redis_client = redis::Client::open(redis_addr).context("Could not connect to redis")?;
    let mut redis_conn = redis_client.get_async_connection().await?;

    // Build the transport from the passed ENV var.
    let (boxed_transport, local_addr) = match (transport, from_env::<SecProtocol>("security")) {
        (Transport::QuicV1, _) => (
            quic::tokio::Transport::new(quic::Config::new(&local_key))
                .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
                .boxed(),
            format!("/ip4/{ip}/udp/0/quic-v1"),
        ),
        (Transport::Tcp, Ok(SecProtocol::Tls)) => (
            tcp::tokio::Transport::new(tcp::Config::new())
                .upgrade(Version::V1Lazy)
                .authenticate(tls::Config::new(&local_key).context("failed to initialise tls")?)
                .multiplex(muxer_protocol_from_env()?)
                .timeout(Duration::from_secs(5))
                .boxed(),
            format!("/ip4/{ip}/tcp/0"),
        ),
        (Transport::Tcp, Ok(SecProtocol::Noise)) => (
            tcp::tokio::Transport::new(tcp::Config::new())
                .upgrade(Version::V1Lazy)
                .authenticate(noise::Config::new(&local_key).context("failed to intialise noise")?)
                .multiplex(muxer_protocol_from_env()?)
                .timeout(Duration::from_secs(5))
                .boxed(),
            format!("/ip4/{ip}/tcp/0"),
        ),
        (Transport::Ws, Ok(SecProtocol::Tls)) => (
            WsConfig::new(tcp::tokio::Transport::new(tcp::Config::new()))
                .upgrade(Version::V1Lazy)
                .authenticate(tls::Config::new(&local_key).context("failed to initialise tls")?)
                .multiplex(muxer_protocol_from_env()?)
                .timeout(Duration::from_secs(5))
                .boxed(),
            format!("/ip4/{ip}/tcp/0/ws"),
        ),
        (Transport::Ws, Ok(SecProtocol::Noise)) => (
            WsConfig::new(tcp::tokio::Transport::new(tcp::Config::new()))
                .upgrade(Version::V1Lazy)
                .authenticate(noise::Config::new(&local_key).context("failed to intialise noise")?)
                .multiplex(muxer_protocol_from_env()?)
                .timeout(Duration::from_secs(5))
                .boxed(),
            format!("/ip4/{ip}/tcp/0/ws"),
        ),
        (Transport::WebRtcDirect, _) => (
            webrtc::tokio::Transport::new(
                local_key,
                webrtc::tokio::Certificate::generate(&mut rand::thread_rng())?,
            )
            .map(|(peer_id, conn), _| (peer_id, StreamMuxerBox::new(conn)))
            .boxed(),
            format!("/ip4/{ip}/udp/0/webrtc-direct"),
        ),
        (Transport::Tcp, Err(_)) => bail!("Missing security protocol for TCP transport"),
        (Transport::Ws, Err(_)) => bail!("Missing security protocol for Websocket transport"),
        (Transport::Webtransport, _) => bail!("Webtransport can only be used with wasm"),
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

    log::info!("Running ping test: {}", swarm.local_peer_id());

    // Run a ping interop test. Based on `is_dialer`, either dial the address
    // retrieved via `listenAddr` key over the redis connection. Or wait to be pinged and have
    // `dialerDone` key ready on the redis connection.
    if is_dialer {
        let result: Vec<String> = redis_conn
            .blpop("listenerAddr", test_timeout.as_secs() as usize)
            .await?;
        let other = result
            .get(1)
            .context("Failed to wait for listener to be ready")?;

        let handshake_start = Instant::now();

        swarm.dial(other.parse::<Multiaddr>()?)?;
        log::info!("Test instance, dialing multiaddress on: {}.", other);

        let rtt = loop {
            if let Some(SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                result: Ok(rtt),
                ..
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
        Ok(())
    } else {
        log::info!(
            "Test instance, listening for incoming connections on: {:?}.",
            local_addr
        );
        let id = swarm.listen_on(local_addr.parse()?)?;

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
                    redis_conn.rpush("listenerAddr", ma).await?;
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
        tokio::time::sleep(test_timeout).await;
        bail!("Test should have been killed by the test runner!");
    }
}

fn muxer_protocol_from_env() -> Result<Either<yamux::Config, mplex::MplexConfig>> {
    Ok(match from_env("muxer")? {
        Muxer::Yamux => Either::Left(yamux::Config::default()),
        Muxer::Mplex => Either::Right(mplex::MplexConfig::new()),
    })
}
