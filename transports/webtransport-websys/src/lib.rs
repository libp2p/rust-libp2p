//! Libp2p WebTransport built on [web-sys](https://rustwasm.github.io/wasm-bindgen/web-sys/index.html)

mod bindings;
mod connection;
mod endpoint;
mod error;
mod fused_js_promise;
mod stream;
mod transport;
mod utils;

pub use self::connection::Connection;
pub use self::error::Error;
pub use self::stream::Stream;
pub use self::transport::{Config, Transport};

#[cfg(test)]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);
