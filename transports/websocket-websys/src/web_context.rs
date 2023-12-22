use std::fmt::Debug;
use std::sync::atomic::{AtomicU8, Ordering};

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::window;

#[wasm_bindgen]
extern "C" {
    #[derive(Debug, Clone)]
    pub(super) type Global;

    #[wasm_bindgen(method, getter, js_name = WorkerGlobalScope)]
    fn worker(this: &Global) -> JsValue;

    #[wasm_bindgen(method, catch, variadic, js_name = setInterval)]
    fn set_interval(
        this: &Global,
        handler: &js_sys::Function,
        timeout: i32,
        arguments: &js_sys::Array,
    ) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(method, catch, js_name = clearInterval)]
    fn clear_interval(this: &Global, handle: JsValue) -> Result<(), JsValue>;
}

#[derive(Debug)]
enum HandleValue {
    Numeric(i32),
    Opaque(JsValue),
}

// we're using additional indirection here, to prevent the caller from knowing
// what type of handle is being returned.
// we also don't implement `From<i32>` or `From<JsValue>` for this type to
// circumvent the creation of invalid handles outside of a context.
#[derive(Debug)]
pub(crate) struct IntervalHandle(HandleValue);

impl IntervalHandle {
    fn from_i32(value: i32) -> Self {
        Self(HandleValue::Numeric(value))
    }

    fn from_value(value: JsValue) -> Self {
        Self(HandleValue::Opaque(value))
    }

    fn into_i32(self) -> Option<i32> {
        match self.0 {
            HandleValue::Numeric(value) => Some(value),
            HandleValue::Opaque(value) => value.as_f64().map(|value| value as i32),
        }
    }

    fn into_value(self) -> JsValue {
        match self.0 {
            HandleValue::Numeric(value) => JsValue::from_f64(value as f64),
            HandleValue::Opaque(value) => value,
        }
    }
}

fn has(value: &JsValue, key: &str) -> bool {
    js_sys::Reflect::has(value, &JsValue::from_str(key)).unwrap_or(false)
}

/// A wrapper around a `u8` that is used to indicate whether the `WebSocket`
/// API is supported.
///
/// * 0x00: Not yet checked
/// * 0x01: Supported
/// * 0x02: Not supported
/// * other: Unknown -> fallback to `0x00`
struct WebSocketSupport(AtomicU8);

impl WebSocketSupport {
    const fn new() -> Self {
        Self(AtomicU8::new(0))
    }

    fn is_supported(&self) -> bool {
        match self.0.load(Ordering::SeqCst) {
            0x01 => true,
            0x02 => false,
            _ => {
                let global = js_sys::global();
                let is_supported = has(&global, "WebSocket");

                let value = if is_supported { 0x01 } else { 0x02 };

                self.0.store(value, Ordering::SeqCst);

                is_supported
            }
        }
    }
}

/// Web context that abstract the window vs web worker API
#[derive(Debug)]
pub(crate) enum WebContext {
    Window(web_sys::Window),
    Worker(web_sys::WorkerGlobalScope),
    Unknown(Global),
}

impl WebContext {
    pub(crate) fn new() -> Option<Self> {
        if let Some(window) = window() {
            return Some(Self::Window(window));
        }

        let global: Global = js_sys::global().unchecked_into();

        if has(&global, "WorkerGlobalScope") {
            return Some(Self::Worker(global.worker().unchecked_into()));
        }

        // check if the `setInterval` and `clearInterval` are available globally
        if has(&global, "setInterval") && has(&global, "clearInterval") {
            return Some(Self::Unknown(global));
        }

        None
    }

    pub(crate) fn is_websocket_supported() -> bool {
        static SUPPORT: WebSocketSupport = WebSocketSupport::new();

        SUPPORT.is_supported()
    }

    /// The `setInterval()` method.
    pub(crate) fn set_interval(
        &self,
        handler: &js_sys::Function,
        timeout: i32,
        arguments: &js_sys::Array,
    ) -> Result<IntervalHandle, JsValue> {
        match self {
            Self::Window(window) => window
                .set_interval_with_callback_and_timeout_and_arguments(handler, timeout, arguments)
                .map(IntervalHandle::from_i32),
            Self::Worker(worker) => worker
                .set_interval_with_callback_and_timeout_and_arguments(handler, timeout, arguments)
                .map(IntervalHandle::from_i32),
            Self::Unknown(global) => global
                .set_interval(handler, timeout, arguments)
                .map(IntervalHandle::from_value),
        }
    }

    /// The `clearInterval()` method.
    ///
    /// # Panics
    ///
    /// This method panics if the handle given invalid.
    /// This only happens whenever the handle was created by another
    /// `WebContext` and that context has a different representation for the
    /// handle.
    ///
    /// In particular this happens when one tries to call `clear_interval` on a
    /// handle that was created in an unknown context in a known context.
    pub(crate) fn clear_interval(&self, handle: IntervalHandle) {
        match self {
            Self::Window(window) => {
                let handle = handle.into_i32().expect("invalid interval handle");

                window.clear_interval_with_handle(handle)
            }
            Self::Worker(worker) => {
                let handle = handle.into_i32().expect("invalid interval handle");

                worker.clear_interval_with_handle(handle)
            }
            Self::Unknown(global) => {
                let handle = handle.into_value();

                global.clear_interval(handle).unwrap()
            }
        }
    }
}
