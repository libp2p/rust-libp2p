// Copyright 2023 Protocol Labs.
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
//! implement the [`libp2p_swarm::NetworkBehaviour`] trait. This struct will automatically try to map the ports externally to internal
//! addresses on the gateway.
//!

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod behaviour;
mod provider;

use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    time::Duration,
};

pub use behaviour::Event;

#[cfg(feature = "async-std")]
pub use provider::async_std;
#[cfg(feature = "tokio")]
pub use provider::tokio;

/// The configuration for UPnP capabilities for libp2p.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Bind address for UDP socket (defaults to all `0.0.0.0`).
    bind_addr: SocketAddr,
    /// Broadcast address for discovery packets (defaults to `239.255.255.250:1900`).
    broadcast_addr: SocketAddr,
    /// Timeout for a search iteration (defaults to 10s).
    timeout: Option<Duration>,
    /// Should the port mappings be temporary (1 hour) or permanent.
    temporary: bool,
}

impl Config {
    /// Creates a new configuration for a UPnP [`libp2p_swarm::NetworkBehaviour`].
    pub fn new() -> Self {
        Self {
            bind_addr: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0)),
            broadcast_addr: "239.255.255.250:1900".parse().unwrap(),
            timeout: Some(Duration::from_secs(10)),
            temporary: true,
        }
    }

    /// Configures the port mappings to be temporary (1 hour) or permanent.
    pub fn temporary(self, temporary: bool) -> Self {
        Self { temporary, ..self }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}
