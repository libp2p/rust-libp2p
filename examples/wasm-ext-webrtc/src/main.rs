mod p2p;

use async_channel as channel;
use leptos::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

// Debugging console log.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

macro_rules! console_log {
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

fn main() {
    // Make sure panics are logged using `console.error`.
    console_error_panic_hook::set_once();

    let app = p2p::P2pApp::new();

    leptos::mount_to_body(|cx| {
        view! { cx,

          <div class="flex flex-col m-2 p-2">
               <label for="peer">Remote MultiAddress:</label>
               <input type="text" id="peer" class="border border-neutral-300 bg-blue-50 rounded p-2 m-2" />
               <button id="connect" class="border bg-slate-100 rounded shadow w-fit p-2 m-2"
                on:click=move |_| {
                    // set_count.update(|n| *n += 1);
                }
               >Dial</button>
           </div>

        }
    });

    // spawn_local(async {
    //     p2p::P2pApp::new().start().await.expect("p2p to start");
    // });
}
