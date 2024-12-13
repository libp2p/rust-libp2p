#![cfg(target_arch = "wasm32")]

use std::{io, time::Duration};

use futures::StreamExt;
use js_sys::Date;
use libp2p::{core::Multiaddr, ping, swarm::SwarmEvent};
use libp2p_webrtc_websys as webrtc_websys;
use wasm_bindgen::prelude::*;
use web_sys::{Document, HtmlElement};

#[wasm_bindgen]
pub async fn run(libp2p_endpoint: String) -> Result<(), JsError> {
    tracing_wasm::set_as_global_default();

    let ping_duration = Duration::from_secs(30);

    let body = Body::from_current_window()?;
    body.append_p(&format!(
        "Let's ping the rust-libp2p server over WebRTC for {:?}:",
        ping_duration
    ))?;

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_wasm_bindgen()
        .with_other_transport(|key| {
            webrtc_websys::Transport::new(webrtc_websys::Config::new(&key))
        })?
        .with_behaviour(|_| ping::Behaviour::new(ping::Config::new()))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(ping_duration))
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
            SwarmEvent::ConnectionClosed {
                cause: Some(cause), ..
            } => {
                tracing::info!("Swarm event: {:?}", cause);

                if let libp2p::swarm::ConnectionError::KeepAliveTimeout = cause {
                    body.append_p("All done with pinging! ")?;

                    break;
                }
                body.append_p(&format!("Connection closed due to: {:?}", cause))?;
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
