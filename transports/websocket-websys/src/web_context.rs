// TODO: remove when https://github.com/rust-lang/rust-clippy/issues/12377 fix lands in stable clippy.
#![allow(clippy::empty_docs)]

use wasm_bindgen::prelude::*;
use web_sys::window;

/// Web context that abstract the window vs web worker API
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

    /// The `setInterval()` method.
    pub(crate) fn set_interval_with_callback_and_timeout_and_arguments(
        &self,
        handler: &::js_sys::Function,
        timeout: i32,
        arguments: &::js_sys::Array,
    ) -> Result<i32, JsValue> {
        match self {
            WebContext::Window(w) => {
                w.set_interval_with_callback_and_timeout_and_arguments(handler, timeout, arguments)
            }
            WebContext::Worker(w) => {
                w.set_interval_with_callback_and_timeout_and_arguments(handler, timeout, arguments)
            }
        }
    }

    /// The `clearInterval()` method.
    pub(crate) fn clear_interval_with_handle(&self, handle: i32) {
        match self {
            WebContext::Window(w) => w.clear_interval_with_handle(handle),
            WebContext::Worker(w) => w.clear_interval_with_handle(handle),
        }
    }
}
