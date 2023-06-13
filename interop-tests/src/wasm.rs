use std::time::Duration;

use anyhow::{bail, ensure, Context, Result};
use futures::StreamExt;
use libp2p::swarm::{keep_alive, SwarmBuilder, SwarmEvent};
use libp2p::{identity, ping, Multiaddr, PeerId};

use crate::{Behaviour, BehaviourEvent, BlpopRequest, Transport};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub async fn run_test(
    transport: &str,
    is_dialer: bool,
    test_timeout_seconds: u64,
    redis_proxy_addr: &str,
) -> Result<String, JsValue> {
    console_error_panic_hook::set_once();
    wasm_logger::init(wasm_logger::Config::default());

    let transport = transport
        .parse()
        .map_err(|e| format!("couldn't parse transport: {e}"))?;

    run_test_impl(transport, is_dialer, test_timeout_seconds, redis_proxy_addr)
        .await
        .map_err(|e| JsValue::from(format!("{e}")))
}

async fn run_test_impl(
    transport: Transport,
    is_dialer: bool,
    test_timeout_seconds: u64,
    redis_proxy_addr: &str,
) -> Result<String> {
    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    // Build the transport from the passed ENV var.
    let boxed_transport = if let Transport::Webtransport = transport {
        libp2p::webtransport_websys::Transport::new(libp2p::webtransport_websys::Config::new(
            &local_key,
        ))
        .boxed()
    } else {
        bail!("Only webtransport supported with wasm")
    };

    let mut swarm = SwarmBuilder::with_wasm_executor(
        boxed_transport,
        Behaviour {
            ping: ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(1))),
            keep_alive: keep_alive::Behaviour,
        },
        local_peer_id,
    )
    .build();

    log::info!("Running ping test: {}", swarm.local_peer_id());

    // Run a ping interop test.
    // Dial the address retrieved via `listenAddr` key over the redis connection.
    // Listening for incoming connections is not supported in browser, so we run only as a dialer.
    ensure!(is_dialer, "Cannot listen for incoming connections from within the browser. Please set is_dialer to true");

    let result: Vec<String> = reqwest::Client::new()
        .post(&format!("http://{}/blpop", redis_proxy_addr))
        .json(&BlpopRequest {
            key: "listenerAddr".to_owned(),
            timeout: test_timeout_seconds,
        })
        .send()
        .await?
        .json()
        .await?;
    let other = result
        .get(1)
        .context("Failed to wait for listener to be ready")?;

    let handshake_start = wasm_timer::Instant::now();

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
