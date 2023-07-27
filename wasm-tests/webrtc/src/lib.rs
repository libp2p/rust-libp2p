use libp2p_core::transport::Transport; // So we can use the Traits that come with it
use libp2p_identity::Keypair;
use libp2p_webrtc::websys::{Config, Connection, Transport as WebRTCTransport}; // So we can dial the server
use multiaddr::Multiaddr;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};
use web_sys::{window, Response};

wasm_bindgen_test_configure!(run_in_browser);

pub const PORT: u16 = 4455;

macro_rules! console_log {
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
    #[wasm_bindgen(js_namespace = console)]
    fn warn(s: &str);
}

#[wasm_bindgen_test]
async fn dial_webrtc_server() {
    let addr = fetch_server_addr().await;
    let keypair = Keypair::generate_ed25519();

    console_log!("Got addr: {:?}", addr);

    let mut transport = WebRTCTransport::new(Config::new(&keypair));
    let connection = match transport.dial(addr) {
        Ok(fut) => fut.await.expect("dial failed"),
        Err(e) => panic!("dial failed: {:?}", e),
    };

    console_log!("Connection established");
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
