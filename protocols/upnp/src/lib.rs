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

//! Implementation of UPnP port mapping for libp2p.
//!
//! This crate provides a `tokio::Behaviour` and `async_std::Behaviour`, depending on the enabled features, which
//! implements the `NetworkBehaviour` trait. This struct will automatically try to map the ports externally to internal
//! addresses on the gateway.
//!

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod behaviour;
mod provider;

use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    time::Duration,
};

#[cfg(feature = "async-std")]
pub use provider::async_std;
#[cfg(feature = "tokio")]
pub use provider::tokio;

/// The configuration for UPnP capabilities for libp2p.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Bind address for UDP socket (defaults to all `0.0.0.0`).
    pub bind_addr: SocketAddr,
    /// Broadcast address for discovery packets (defaults to `239.255.255.250:1900`).
    pub broadcast_addr: SocketAddr,
    /// Timeout for a search iteration (defaults to 10s).
    pub timeout: Option<Duration>,
    /// Should the port mappings be temporary or permanent.
    pub permanent: bool,
}

impl Config {
    /// Creates a new configuration for a UPnP transport:
    /// * Bind address for UDP socket is `0.0.0.0`.
    /// See [`Config::bind_addr`].
    /// * Broadcast address for discovery packets is `239.255.255.250:1900`.
    /// See [`Config::broadcast_address`].
    /// * Timeout for a search iteration is 10s.
    /// See [`Config::timeout`].
    pub fn new() -> Self {
        Self {
            bind_addr: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            broadcast_addr: "239.255.255.250:1900".parse().unwrap(),
            timeout: Some(Duration::from_secs(10)),
            permanent: false,
        }
    }

    /// Configures the Bind address for UDP socket.
    pub fn bind_addr(self, bind_addr: SocketAddr) -> Self {
        Self { bind_addr, ..self }
    }

    /// Configures the Broadcast address for discovery packets.
    pub fn broadcast_address(self, broadcast_addr: SocketAddr) -> Self {
        Self {
            broadcast_addr,
            ..self
        }
    }

    /// Configures the timeout for search iteration.
    pub fn timeout(self, timeout: Option<Duration>) -> Self {
        Self { timeout, ..self }
    }

    /// Configures if the port mappings should be temporary or permanent.
    pub fn permanent(self, permanent: bool) -> Self {
        Self { permanent, ..self }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}
