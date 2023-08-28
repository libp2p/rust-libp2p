use crate::error::PingerError;
use futures::{channel, SinkExt, StreamExt};
use libp2p::core::Multiaddr;
use libp2p::identity::{Keypair, PeerId};
use libp2p::ping;
use libp2p::swarm::{keep_alive, NetworkBehaviour, SwarmBuilder, SwarmEvent};
use std::convert::From;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

// The PORT that the server serves their Multiaddr
pub const PORT: u16 = 4455;

pub(crate) async fn start_pinger(
    mut sendr: channel::mpsc::Sender<Result<f32, PingerError>>,
) -> Result<(), PingerError> {
    let addr = fetch_server_addr().await;

    log::trace!("Got addr {} from server", addr);
    let local_key = Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    let mut swarm = SwarmBuilder::with_wasm_executor(
        libp2p_webrtc_websys::Transport::new(libp2p_webrtc_websys::Config::new(&local_key)).boxed(),
        Behaviour {
            ping: ping::Behaviour::new(ping::Config::new()),
            keep_alive: keep_alive::Behaviour,
        },
        local_peer_id,
    )
    .build();

    log::info!("Running pinger with peer_id: {}", swarm.local_peer_id());

    log::info!("Dialing {}", addr);

    swarm.dial(addr.parse::<Multiaddr>()?)?;

    loop {
        match swarm.next().await.unwrap() {
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event { result: Err(e), .. })) => {
                log::error!("Ping failed: {:?}", e);
                sendr.send(Err(e.into())).await?;
            }
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                peer,
                result: Ok(rtt),
                ..
            })) => {
                log::info!("Ping successful: RTT: {rtt:?}, from {peer}");
                sendr.send(Ok(rtt.as_secs_f32())).await?;
            }
            evt => log::info!("Swarm event: {:?}", evt),
        }
    }
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
}

/// Helper that returns the multiaddress of echo-server
///
/// It fetches the multiaddress via HTTP request to
/// 127.0.0.1:4455.
pub async fn fetch_server_addr() -> String {
    let url = format!("http://127.0.0.1:{}/", PORT);
    let window = web_sys::window().expect("no global `window` exists");

    let value = match JsFuture::from(window.fetch_with_str(&url)).await {
        Ok(value) => value,
        Err(err) => {
            log::error!("fetch failed: {:?}", err);
            return "".to_string();
        }
    };
    let resp = match value.dyn_into::<web_sys::Response>() {
        Ok(resp) => resp,
        Err(err) => {
            log::error!("fetch response failed: {:?}", err);
            return "".to_string();
        }
    };

    let text = match resp.text() {
        Ok(text) => text,
        Err(err) => {
            log::error!("fetch text failed: {:?}", err);
            return "".to_string();
        }
    };
    let text = match JsFuture::from(text).await {
        Ok(text) => text,
        Err(err) => {
            log::error!("convert future failed: {:?}", err);
            return "".to_string();
        }
    };

    match text.as_string().filter(|s| !s.is_empty()) {
        Some(text) => text,
        None => {
            log::error!("fetch text is empty");
            "".to_string()
        }
    }
}
