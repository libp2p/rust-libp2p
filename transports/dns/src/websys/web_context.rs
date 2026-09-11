use js_sys::Promise;
use wasm_bindgen::prelude::*;
use web_sys::{Request, window};

/// Web context that abstracts the `window` vs web worker global scope, so that
/// DoH lookups work both on the main thread and inside workers.
#[derive(Debug)]
pub(crate) enum WebContext {
    Window(web_sys::Window),
    Worker(web_sys::WorkerGlobalScope),
}

impl WebContext {
    pub(crate) fn new() -> Option<Self> {
        match window() {
            Some(window) => Some(Self::Window(window)),
            None => {
                #[wasm_bindgen]
                extern "C" {
                    type Global;

                    #[wasm_bindgen(method, getter, js_name = WorkerGlobalScope)]
                    fn worker(this: &Global) -> JsValue;
                }
                let global: Global = js_sys::global().unchecked_into();
                if !global.worker().is_undefined() {
                    Some(Self::Worker(global.unchecked_into()))
                } else {
                    None
                }
            }
        }
    }

    /// The `fetch()` method.
    pub(crate) fn fetch_with_request(&self, request: &Request) -> Promise {
        match self {
            WebContext::Window(w) => w.fetch_with_request(request),
            WebContext::Worker(w) => w.fetch_with_request(request),
        }
    }
}
