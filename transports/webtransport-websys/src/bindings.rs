//! This file is an extract from `web-sys` crate. It is a temporary
//! solution until `web_sys::WebTransport` and related structs get stabilized.
//!
//! Only the methods that are used by this crate are extracted.

#![allow(clippy::all)]
use js_sys::{Object, Promise, Reflect};
use wasm_bindgen::prelude::*;
use web_sys::{ReadableStream, WritableStream};

// WebTransport bindings
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = Object, js_name = WebTransport, typescript_type = "WebTransport")]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type WebTransport;

    #[wasm_bindgen(structural, method, getter, js_class = "WebTransport", js_name = ready)]
    pub fn ready(this: &WebTransport) -> Promise;

    #[wasm_bindgen(structural, method, getter, js_class = "WebTransport", js_name = closed)]
    pub fn closed(this: &WebTransport) -> Promise;

    #[wasm_bindgen(structural, method, getter, js_class = "WebTransport" , js_name = incomingBidirectionalStreams)]
    pub fn incoming_bidirectional_streams(this: &WebTransport) -> ReadableStream;

    #[wasm_bindgen(catch, constructor, js_class = "WebTransport")]
    pub fn new(url: &str) -> Result<WebTransport, JsValue>;

    #[wasm_bindgen(catch, constructor, js_class = "WebTransport")]
    pub fn new_with_options(
        url: &str,
        options: &WebTransportOptions,
    ) -> Result<WebTransport, JsValue>;

    #[wasm_bindgen(method, structural, js_class = "WebTransport", js_name = close)]
    pub fn close(this: &WebTransport);

    #[wasm_bindgen (method, structural, js_class = "WebTransport", js_name = createBidirectionalStream)]
    pub fn create_bidirectional_stream(this: &WebTransport) -> Promise;
}

// WebTransportBidirectionalStream bindings
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = Object, js_name = WebTransportBidirectionalStream, typescript_type = "WebTransportBidirectionalStream")]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type WebTransportBidirectionalStream;

    #[wasm_bindgen(structural, method, getter, js_class = "WebTransportBidirectionalStream", js_name = readable)]
    pub fn readable(this: &WebTransportBidirectionalStream) -> WebTransportReceiveStream;

    #[wasm_bindgen(structural, method, getter, js_class = "WebTransportBidirectionalStream", js_name = writable)]
    pub fn writable(this: &WebTransportBidirectionalStream) -> WebTransportSendStream;
}

// WebTransportReceiveStream bindings
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = ReadableStream, extends = Object, js_name = WebTransportReceiveStream, typescript_type = "WebTransportReceiveStream")]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type WebTransportReceiveStream;
}

// WebTransportSendStream bindings
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = WritableStream, extends = Object, js_name = WebTransportSendStream, typescript_type = "WebTransportSendStream")]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type WebTransportSendStream;
}

// WebTransportOptions bindings
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = Object, js_name = WebTransportOptions)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type WebTransportOptions;
}

impl WebTransportOptions {
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut ret: Self = JsCast::unchecked_into(Object::new());
        ret
    }

    pub fn server_certificate_hashes(&mut self, val: &JsValue) -> &mut Self {
        let r = ::js_sys::Reflect::set(
            self.as_ref(),
            &JsValue::from("serverCertificateHashes"),
            &JsValue::from(val),
        );
        debug_assert!(
            r.is_ok(),
            "setting properties should never fail on our dictionary objects"
        );
        let _ = r;
        self
    }
}

// WebTransportHash bindings
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = Object, js_name = WebTransportHash)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type WebTransportHash;
}

impl WebTransportHash {
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut ret: Self = JsCast::unchecked_into(Object::new());
        ret
    }

    pub fn algorithm(&mut self, val: &str) -> &mut Self {
        let r = Reflect::set(
            self.as_ref(),
            &JsValue::from("algorithm"),
            &JsValue::from(val),
        );
        debug_assert!(
            r.is_ok(),
            "setting properties should never fail on our dictionary objects"
        );
        let _ = r;
        self
    }

    pub fn value(&mut self, val: &::js_sys::Object) -> &mut Self {
        let r = Reflect::set(self.as_ref(), &JsValue::from("value"), &JsValue::from(val));
        debug_assert!(
            r.is_ok(),
            "setting properties should never fail on our dictionary objects"
        );
        let _ = r;
        self
    }
}
