use async_channel::bounded;
use leptos::*;
use pinger::start_pinger;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use wasm_bindgen_futures::JsFuture;
use web_sys::{window, Response};

pub mod pinger;

// The PORT that the server serves their Multiaddr
pub const PORT: u16 = 4455;

/// Our main Leptos App component
#[component]
pub fn App(cx: Scope) -> impl IntoView {
    // create a mpsc channel to get pings to from the pinger
    let (sendr, recvr) = bounded::<f32>(2);

    // create Leptos signal to update the Pings diplayed
    let (number_of_pings, set_number_of_pings) = create_signal(cx, Vec::new());

    // start the pinger, pass in our sender
    spawn_local(async move {
        match start_pinger(sendr).await {
            Ok(_) => log::info!("Pinger finished"),
            Err(e) => log::error!("Pinger error: {:?}", e),
        };
    });

    // update number of pings signal each time out receiver receives an update
    spawn_local(async move {
        loop {
            match recvr.recv().await {
                Ok(rtt) => {
                    // set rtt and date time stamp
                    let now = chrono::Utc::now().timestamp();
                    log::info!("[{now:?}] Got RTT: {rtt:?} ms");
                    set_number_of_pings.update(move |pings| pings.insert(0, (now, rtt)));
                }
                Err(e) => log::error!("Pinger channel closed: {:?}", e),
            }
        }
    });

    // Build our DOM HTML
    view! { cx,
        <h1>"Rust Libp2p WebRTC Demo"</h1>
        <h2>"Pinging every 15 seconds. Open Browser console for more logging details."</h2>
        <ul>
            <For
                each=number_of_pings
                // the key is the timestamp
                key=|ping| ping.0
                view=move |cx, (stamp, rtt)| {
                    view! { cx,
                        <li>
                        <span>{
                            chrono::NaiveDateTime::from_timestamp_opt(stamp, 0)
                                .expect("timestamp is valid")
                                .format("%Y-%m-%d %H:%M:%S")
                                .to_string()
                            }</span>" in "
                           {rtt} "ms"
                        </li>
                    }
                }
            />
        </ul>
    }
}

/// Helper that returns the multiaddress of echo-server
///
/// It fetches the multiaddress via HTTP request to
/// 127.0.0.1:4455.
pub async fn fetch_server_addr() -> String {
    let url = format!("http://127.0.0.1:{}/", PORT);
    let window = window().expect("no global `window` exists");

    let value = match JsFuture::from(window.fetch_with_str(&url)).await {
        Ok(value) => value,
        Err(err) => {
            log::error!("fetch failed: {:?}", err);
            return "".to_string();
        }
    };
    let resp = match value.dyn_into::<Response>() {
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
