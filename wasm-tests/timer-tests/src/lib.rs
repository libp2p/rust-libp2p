//! Browser tests for the timers used by the `Swarm` and by protocols.
//!
//! On `wasm32`, `libp2p` enables the `wasm-bindgen` feature of `futures-timer`, which backs its
//! `Delay` with `gloo-timers`, i.e. with the browser's `setTimeout`. The `Swarm` test also runs
//! connections on the `wasm-bindgen-futures` executor.
#![cfg(target_arch = "wasm32")]

use std::{future::poll_fn, task::Poll, time::Duration};

use futures::StreamExt;
use futures_timer::Delay;
use libp2p::{
    core::{Transport as _, transport::MemoryTransport, upgrade::Version},
    identity::Keypair,
    multiaddr::Protocol,
    plaintext,
    swarm::{Config, ConnectionError, Swarm, SwarmEvent, dummy},
    yamux,
};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_browser);

const TIMEOUT: Duration = Duration::from_millis(100);

#[wasm_bindgen_test]
pub async fn delay_fires() {
    let mut delay = Delay::new(TIMEOUT);
    assert!(futures::poll!(&mut delay).is_pending());

    // A much shorter delay fires first, while `delay` is still pending.
    Delay::new(TIMEOUT / 10).await;
    assert!(futures::poll!(&mut delay).is_pending());

    delay.await;
}

#[wasm_bindgen_test]
pub async fn idle_connection_is_closed_after_timeout() {
    // Only the dialer closes the connection when it becomes idle.
    let mut listener = new_swarm(Duration::from_secs(60));
    let mut dialer = new_swarm(TIMEOUT);

    listener.listen_on(Protocol::Memory(0).into()).unwrap();
    let address = loop {
        if let SwarmEvent::NewListenAddr { address, .. } = listener.select_next_some().await {
            break address;
        }
    };
    dialer.dial(address).unwrap();

    let cause = poll_fn(|cx| {
        while listener.poll_next_unpin(cx).is_ready() {}

        while let Poll::Ready(Some(event)) = dialer.poll_next_unpin(cx) {
            if let SwarmEvent::ConnectionClosed { cause, .. } = event {
                return Poll::Ready(cause);
            }
        }

        Poll::Pending
    })
    .await;

    assert!(
        matches!(cause, Some(ConnectionError::KeepAliveTimeout)),
        "unexpected cause: {cause:?}"
    );
}

fn new_swarm(idle_connection_timeout: Duration) -> Swarm<dummy::Behaviour> {
    let identity = Keypair::generate_ed25519();
    let transport = MemoryTransport::default()
        .upgrade(Version::V1)
        .authenticate(plaintext::Config::new(&identity))
        .multiplex(yamux::Config::default())
        .boxed();

    Swarm::new(
        transport,
        dummy::Behaviour,
        identity.public().to_peer_id(),
        Config::with_wasm_executor().with_idle_connection_timeout(idle_connection_timeout),
    )
}
