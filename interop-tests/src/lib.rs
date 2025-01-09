use std::{str::FromStr, time::Duration};

use anyhow::{bail, Context, Result};
use futures::{FutureExt, StreamExt};
use libp2p::{
    identify,
    identity::Keypair,
    ping,
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr,
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

mod arch;

use arch::{build_swarm, init_logger, Instant, RedisClient};

pub async fn run_test(
    transport: &str,
    ip: &str,
    is_dialer: bool,
    test_timeout_seconds: u64,
    redis_addr: &str,
    sec_protocol: Option<String>,
    muxer: Option<String>,
) -> Result<Report> {
    init_logger();

    let test_timeout = Duration::from_secs(test_timeout_seconds);
    let transport = transport.parse().context("Couldn't parse transport")?;
    let sec_protocol = sec_protocol
        .map(|sec_protocol| {
            sec_protocol
                .parse()
                .context("Couldn't parse security protocol")
        })
        .transpose()?;
    let muxer = muxer
        .map(|sec_protocol| {
            sec_protocol
                .parse()
                .context("Couldn't parse muxer protocol")
        })
        .transpose()?;

    let redis_client = RedisClient::new(redis_addr).context("Could not connect to redis")?;

    // Build the transport from the passed ENV var.
    let (mut swarm, local_addr) =
        build_swarm(ip, transport, sec_protocol, muxer, build_behaviour).await?;

    tracing::info!(local_peer=%swarm.local_peer_id(), "Running ping test");

    // See https://github.com/libp2p/rust-libp2p/issues/4071.
    #[cfg(not(target_arch = "wasm32"))]
    let maybe_id = if transport == Transport::WebRtcDirect {
        Some(swarm.listen_on(local_addr.parse()?)?)
    } else {
        None
    };
    #[cfg(target_arch = "wasm32")]
    let maybe_id = None;

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
            tracing::info!(listener=%other, "Test instance, dialing multiaddress");

            let rtt = loop {
                if let Some(SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                    result: Ok(rtt),
                    ..
                }))) = swarm.next().await
                {
                    tracing::info!(?rtt, "Ping successful");
                    break rtt.as_micros() as f32 / 1000.;
                }
            };

            let handshake_plus_ping = handshake_start.elapsed().as_micros() as f32 / 1000.;
            Ok(Report {
                handshake_plus_one_rtt_millis: handshake_plus_ping,
                ping_rtt_millis: rtt,
            })
        }
        false => {
            // Listen if we haven't done so already.
            // This is a hack until https://github.com/libp2p/rust-libp2p/issues/4071 is fixed at which point we can do this unconditionally here.
            let id = match maybe_id {
                None => swarm.listen_on(local_addr.parse()?)?,
                Some(id) => id,
            };

            tracing::info!(
                address=%local_addr,
                "Test instance, listening for incoming connections on address"
            );

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
                        let ma = format!("{address}/p2p/{}", swarm.local_peer_id());
                        redis_client.rpush("listenerAddr", ma.clone()).await?;
                        break;
                    }
                }
            }

            // Drive Swarm while we await for `dialerDone` to be ready.
            futures::future::select(
                async move {
                    loop {
                        let event = swarm.next().await.unwrap();

                        tracing::debug!("{event:?}");
                    }
                }
                .boxed(),
                arch::sleep(test_timeout),
            )
            .await;

            // The loop never ends so if we get here, we hit the timeout.
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
    test_timeout_secs: u64,
    base_url: &str,
    sec_protocol: Option<String>,
    muxer: Option<String>,
) -> Result<(), JsValue> {
    let result = run_test(
        transport,
        ip,
        is_dialer,
        test_timeout_secs,
        base_url,
        sec_protocol,
        muxer,
    )
    .await;
    tracing::info!(?result, "Sending test result");
    reqwest::Client::new()
        .post(&format!("http://{}/results", base_url))
        .json(&result.map_err(|e| e.to_string()))
        .send()
        .await?
        .error_for_status()
        .map_err(|e| format!("Sending test result failed: {e}"))?;

    Ok(())
}

/// A request to redis proxy that will pop the value from the list
/// and will wait for it being inserted until a timeout is reached.
#[derive(serde::Deserialize, serde::Serialize)]
pub struct BlpopRequest {
    pub key: String,
    pub timeout: u64,
}

/// A report generated by the test
#[derive(Copy, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Report {
    #[serde(rename = "handshakePlusOneRTTMillis")]
    handshake_plus_one_rtt_millis: f32,
    #[serde(rename = "pingRTTMilllis")]
    ping_rtt_millis: f32,
}

/// Supported transports by rust-libp2p.
#[derive(Clone, Copy, Debug, PartialEq)]
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
pub(crate) struct Behaviour {
    ping: ping::Behaviour,
    identify: identify::Behaviour,
}

pub(crate) fn build_behaviour(key: &Keypair) -> Behaviour {
    Behaviour {
        ping: ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(1))),
        // Need to include identify until https://github.com/status-im/nim-libp2p/issues/924 is resolved.
        identify: identify::Behaviour::new(identify::Config::new(
            "/interop-tests".to_owned(),
            key.public(),
        )),
    }
}
