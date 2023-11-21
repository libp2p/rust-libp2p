#![cfg(target_arch = "wasm32")]

use futures::StreamExt;
use js_sys::Date;
use libp2p::core::Multiaddr;
use libp2p::ping;
use libp2p::swarm::SwarmEvent;
use libp2p_webrtc_websys as webrtc_websys;
use std::io;
use std::time::Duration;
use wasm_bindgen::prelude::*;
use web_sys::{Document, HtmlElement};

#[wasm_bindgen]
pub async fn run(libp2p_endpoint: String) -> Result<(), JsError> {
    wasm_logger::init(wasm_logger::Config::default());

    let body = Body::from_current_window()?;
    body.append_p("Let's ping the WebRTC Server!")?;

    let mut swarm =
        libp2p::SwarmBuilder::with_new_identity()
            .with_wasm_bindgen()
            .with_other_transport(
                |key| webrtc_websys::Transport::new(webrtc_websys::Config::new(&key))
            )?
            .with_behaviour(|_| ping::Behaviour::new(ping::Config::new()))?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(5)))
            .build();

    let addr = libp2p_endpoint.parse::<Multiaddr>()?;
    tracing::info!("Dialing {addr}");
    swarm.dial(addr)?;

    loop {
        match swarm.next().await.unwrap() {
            SwarmEvent::Behaviour(ping::Event { result: Err(e), .. }) => {
                tracing::error!("Ping failed: {:?}", e);

                break;
            }
            SwarmEvent::Behaviour(ping::Event {
                peer,
                result: Ok(rtt),
                ..
            }) => {
                tracing::info!("Ping successful: RTT: {rtt:?}, from {peer}");
                body.append_p(&format!("RTT: {rtt:?} at {}", Date::new_0().to_string()))?;
            }
            evt => tracing::info!("Swarm event: {:?}", evt),
        }
    }

    Ok(())
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

fn js_error(msg: &str) -> JsError {
    io::Error::new(io::ErrorKind::Other, msg).into()
}
