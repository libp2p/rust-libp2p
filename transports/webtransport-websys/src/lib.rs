//! Libp2p WebTransport built on [web-sys](https://wasm-bindgen.github.io/wasm-bindgen/contributing/web-sys/index.html)

#![allow(unexpected_cfgs)]

mod bindings;
mod connection;
mod endpoint;
mod error;
mod fused_js_promise;
mod stream;
mod transport;
mod utils;

pub use self::{
    connection::Connection,
    error::Error,
    stream::Stream,
    transport::{Config, Transport},
};
