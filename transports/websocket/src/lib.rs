// Copyright 2017 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

#![recursion_limit = "512"]

//! Implementation of the libp2p `Transport` trait for Websockets.
//!
//! See the documentation of `swarm` and of libp2p in general to learn how to use the `Transport`
//! trait.
//!
//! This library is used in a different way depending on whether you are compiling for emscripten
//! or for a different operating system.
//!
//! # Emscripten
//!
//! On emscripten, you can create a `BrowserWsConfig` object with `BrowserWsConfig::new()`. It can
//! then be used as a transport.
//!
//! Listening on a websockets multiaddress isn't supported on emscripten. Dialing a multiaddress
//! which uses `ws` on top of TCP/IP will automatically use the `XMLHttpRequest` Javascript object.
//!
//! ```ignore
//! use libp2p_websocket::BrowserWsConfig;
//!
//! let ws_config = BrowserWsConfig::new();
//! // let _ = ws_config.dial("/ip4/40.41.42.43/tcp/12345/ws".parse().unwrap());
//! ```
//!
//! # Other operating systems
//!
//! On other operating systems, this library doesn't open any socket by itself. Instead it must be
//! plugged on top of another implementation of `Transport` such as TCP/IP.
//!
//! This underlying transport must be put inside a `WsConfig` object through the
//! `WsConfig::new()` function.
//!
//! ```
//! extern crate libp2p_core;
//! extern crate libp2p_tcp;
//! extern crate libp2p_websocket;
//!
//! use libp2p_core::{Multiaddr, Transport};
//! use libp2p_tcp::TcpConfig;
//! use libp2p_websocket::WsConfig;
//!
//! # fn main() {
//! let ws_config = WsConfig::new(TcpConfig::new());
//! # return;
//! let _ = ws_config.dial("/ip4/40.41.42.43/tcp/12345/ws".parse().unwrap());
//! # }
//! ```
//!

extern crate futures;
extern crate libp2p_core as swarm;
#[macro_use]
extern crate log;
extern crate multiaddr;
extern crate rw_stream_sink;
extern crate tokio_io;

#[cfg(any(target_os = "emscripten", target_os = "unknown"))]
#[macro_use]
extern crate stdweb;
#[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
extern crate websocket;

#[cfg(any(target_os = "emscripten", target_os = "unknown"))]
mod browser;
#[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
mod desktop;

#[cfg(any(target_os = "emscripten", target_os = "unknown"))]
pub use self::browser::{BrowserWsConfig, BrowserWsConn};
#[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
pub use self::desktop::WsConfig;
