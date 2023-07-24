use futures::channel::oneshot;
use futures::{AsyncReadExt, AsyncWriteExt};
use getrandom::getrandom;
use libp2p::core::{StreamMuxer, Transport as _};
use libp2p::noise;
use libp2p_identity::{Keypair, PeerId};
// use libp2p_webtransport_websys::{Config, Connection, Error, Stream, Transport};
use multiaddr::{Multiaddr, Protocol};
use multihash::Multihash;
use std::future::poll_fn;
use std::pin::Pin;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{window, Response};

wasm_bindgen_test_configure!(run_in_browser);

pub const PORT: u16 = 4455;

#[wasm_bindgen_test]
async fn connect_without_peer_id() {
    let mut addr = fetch_server_addr().await;
    let keypair = Keypair::generate_ed25519();

    // eprintln
    eprintln!("addr: {:?}", addr);
    println!("addr: {:?}", addr);
}

/// Helper that returns the multiaddress of echo-server
///
/// It fetches the multiaddress via HTTP request to
/// 127.0.0.1:4455.
async fn fetch_server_addr() -> Multiaddr {
    let url = format!("http://127.0.0.1:{}/", PORT);
    let window = window().expect("failed to get browser window");

    let value = JsFuture::from(window.fetch_with_str(&url))
        .await
        .expect("fetch failed");
    let resp = value.dyn_into::<Response>().expect("cast failed");

    let text = resp.text().expect("text failed");
    let text = JsFuture::from(text).await.expect("text promise failed");

    text.as_string()
        .filter(|s| !s.is_empty())
        .expect("response not a text")
        .parse()
        .unwrap()
}
