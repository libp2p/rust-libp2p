use std::str::FromStr;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures::StreamExt;
use libp2p::swarm::{keep_alive, NetworkBehaviour, SwarmEvent};
use libp2p::{identity, ping, Multiaddr, PeerId};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

mod arch;

use arch::{build_transport, init_logger, swarm_builder, Instant, RedisClient};

pub async fn run_test(
    transport: Transport,
    ip: &str,
    is_dialer: bool,
    test_timeout: Duration,
    redis_addr: &str,
) -> Result<String> {
    init_logger();

    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    let redis_client = RedisClient::new(redis_addr).context("Could not connect to redis")?;

    // Build the transport from the passed ENV var.
    let (boxed_transport, local_addr) = build_transport(local_key, ip, transport)?;
    let mut swarm = swarm_builder(
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
    match is_dialer {
        true => {
            let result: Vec<String> = redis_client
                .blpop("listenerAddr", test_timeout.as_secs())
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
            Ok(format!(
                r#"{{"handshakePlusOneRTTMillis": {handshake_plus_ping:.1}, "pingRTTMilllis": {rtt:.1}}}"#
            ))
        }
        #[cfg(target_arch = "wasm32")]
        false => bail!("Cannot run as listener in browser, please change is_dialer to true"),
        #[cfg(not(target_arch = "wasm32"))]
        false => {
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
                        redis_client.rpush("listenerAddr", ma).await?;
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
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn run_test_wasm(
    transport: &str,
    ip: &str,
    is_dialer: bool,
    test_timeout_seconds: u64,
    redis_proxy_addr: &str,
) -> Result<String, JsValue> {
    let test_timeout = Duration::from_secs(test_timeout_seconds);
    let transport = transport
        .parse()
        .map_err(|e| format!("Couldn't parse transport: {e}"))?;

    let result = run_test(transport, ip, is_dialer, test_timeout, redis_proxy_addr)
        .await
        .map_err(|e| format!("Running tests failed: {e}"))?;

    Ok(result)
}

/// A request to redis proxy that will pop the value from the list
/// and will wait for it being inserted until a timeout is reached.
#[derive(serde::Deserialize, serde::Serialize)]
pub struct BlpopRequest {
    pub key: String,
    pub timeout: u64,
}

/// Supported transports by rust-libp2p.
#[derive(Clone, Debug)]
pub enum Transport {
    Tcp,
    QuicV1,
    WebRtcDirect,
    Ws,
    Webtransport,
}

impl FromStr for Transport {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "tcp" => Self::Tcp,
            "quic-v1" => Self::QuicV1,
            "webrtc-direct" => Self::WebRtcDirect,
            "ws" => Self::Ws,
            "webtransport" => Self::Webtransport,
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
    std::env::var(env_var)
        .with_context(|| format!("{env_var} environment variable is not set"))?
        .parse()
        .map_err(Into::into)
}
