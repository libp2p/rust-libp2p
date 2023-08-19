mod error;
mod pinger;
mod utils;

use crate::error::PingerError;
use futures::channel;
use futures::StreamExt;
use js_sys::Date;
use pinger::start_pinger;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[wasm_bindgen]
extern "C" {
    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

macro_rules! console_log {
    // Note that this is using the `log` function imported above during
    // `bare_bones`
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

#[wasm_bindgen(start)]
pub fn run() -> Result<(), JsValue> {
    console_log!("Starting main()");

    match console_log::init_with_level(log::Level::Info) {
        Ok(_) => log::info!("Console logging initialized"),
        Err(_) => log::info!("Console logging already initialized"),
    };

    // Use `web_sys`'s global `window` function to get a handle on the global
    // window object.
    let window = web_sys::window().expect("no global `window` exists");
    let document = window.document().expect("should have a document on window");
    let body = document.body().expect("document should have a body");

    // Manufacture the element we're gonna append
    let val = document.create_element("p")?;
    val.set_text_content(Some("Let's ping the WebRTC Server!"));

    body.append_child(&val)?;

    // create a mpsc channel to get pings to from the pinger
    let (sendr, mut recvr) = channel::mpsc::unbounded::<Result<f32, PingerError>>();

    log::info!("Spawn a pinger");
    console_log!("Spawn a pinger main()");

    // start the pinger, pass in our sender
    spawn_local(async move {
        log::info!("Spawning pinger");
        match start_pinger(sendr).await {
            Ok(_) => log::info!("Pinger finished"),
            Err(e) => log::info!("Pinger error: {:?}", e),
        };
    });

    // loop on recvr await, appending to the DOM with date and RTT when we get it
    spawn_local(async move {
        loop {
            match recvr.next().await {
                Some(Ok(rtt)) => {
                    log::info!("Got RTT: {}", rtt);
                    let val = document
                        .create_element("p")
                        .expect("should create a p elem");
                    val.set_text_content(Some(&format!(
                        "RTT: {}ms at {}",
                        rtt,
                        Date::new_0().to_string()
                    )));
                    body.append_child(&val).expect("should append body elem");
                }
                Some(Err(e)) => log::info!("Error: {:?}", e),
                None => log::info!("Recvr channel closed"),
            }
        }
    });

    Ok(())
}
