//! Libp2p WebTransport built on [web-sys](https://rustwasm.github.io/wasm-bindgen/web-sys/index.html)

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
