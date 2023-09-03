#![cfg(target_arch = "wasm32")]

use futures::StreamExt;
use js_sys::Date;
use libp2p::core::Multiaddr;
use libp2p::identity::{Keypair, PeerId};
use libp2p::ping;
use libp2p::swarm::{keep_alive, NetworkBehaviour, SwarmBuilder, SwarmEvent};
use std::convert::From;
use std::io;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Document, HtmlElement};

#[wasm_bindgen(start)]
pub async fn run() -> Result<(), JsError> {
    wasm_logger::init(wasm_logger::Config::default());
    let body = Body::from_current_window()?;

    body.append_p("Let's ping the WebRTC Server!")?;

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

    log::info!("Initialize swarm with identity: {local_peer_id}");

    let addr = fetch_server_addr().await?;
    log::info!("Dialing {addr}");
    swarm.dial(addr)?;

    loop {
        match swarm.next().await.unwrap() {
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event { result: Err(e), .. })) => {
                log::error!("Ping failed: {:?}", e);

                break;
            }
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping::Event {
                peer,
                result: Ok(rtt),
                ..
            })) => {
                log::info!("Ping successful: RTT: {rtt:?}, from {peer}");
                body.append_p(&format!("RTT: {rtt:?} at {}", Date::new_0().to_string()))?;
            }
            evt => log::info!("Swarm event: {:?}", evt),
        }
    }

    Ok(())
}

#[derive(NetworkBehaviour)]
struct Behaviour {
    ping: ping::Behaviour,
    keep_alive: keep_alive::Behaviour,
}

/// Convenience wrapper around the current document body
struct Body {
    body: HtmlElement,
    document: Document,
}

impl Body {
    fn from_current_window() -> Result<Self, JsError> {
        // Use `web_sys`'s global `window` function to get a handle on the global
        // window object.
        let document = web_sys::window()
            .ok_or(js_error("no global `window` exists"))?
            .document()
            .ok_or(js_error("should have a document on window"))?;
        let body = document
            .body()
            .ok_or(js_error("document should have a body"))?;

        Ok(Self { body, document })
    }

    fn append_p(&self, msg: &str) -> Result<(), JsError> {
        let val = self
            .document
            .create_element("p")
            .map_err(|_| js_error("failed to create <p>"))?;
        val.set_text_content(Some(msg));
        self.body
            .append_child(&val)
            .map_err(|_| js_error("failed to append <p>"))?;

        Ok(())
    }
}

/// Helper that returns the multiaddress of echo-server
///
/// It fetches the multiaddress via HTTP request to from our serving host.
async fn fetch_server_addr() -> Result<Multiaddr, JsError> {
    let window = web_sys::window().expect("no global `window` exists");

    let response_promise = JsFuture::from(window.fetch_with_str("/address"))
        .await
        .map_err(|_| js_error("fetch failed"))?
        .dyn_into::<web_sys::Response>()
        .map_err(|_| js_error("cast to `Response` failed"))?
        .text()
        .map_err(|_| js_error("response is not text"))?;

    let value = JsFuture::from(response_promise)
        .await
        .map_err(|_| js_error("failed to retrieve body as text"))?
        .as_string()
        .filter(|s| !s.is_empty())
        .ok_or(js_error("fetch text is empty"))?
        .parse::<Multiaddr>()?;

    Ok(value)
}

fn js_error(msg: &str) -> JsError {
    io::Error::new(io::ErrorKind::Other, msg).into()
}
