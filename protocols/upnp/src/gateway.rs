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

use std::{
    error::Error,
    fmt,
    net::{Ipv4Addr, SocketAddrV4},
    sync::Arc,
    time::Duration,
};

use crate::Config;
use async_trait::async_trait;

#[cfg(feature = "async-std")]
pub mod async_std;

#[cfg(feature = "tokio")]
pub mod tokio;

/// Protocols available for port mapping.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Protocol::Tcp => f.write_str("tcp"),
            Protocol::Udp => f.write_str("udp"),
        }
    }
}

/// The interface for non-blocking UPnP I/O providers.
#[async_trait]
pub trait Gateway: Sized + Send + Sync {
    /// Search for the gateway endpoint on the local network.
    async fn search(config: Config) -> Result<(Self, Ipv4Addr), Box<dyn Error>>;

    /// Add the input port mapping on the gateway so that traffic is forwarded to the input address.
    async fn add_port(
        _: Arc<Self>,
        protocol: Protocol,
        addr: SocketAddrV4,
        duration: Duration,
    ) -> Result<(), Box<dyn Error>>;

    /// Remove port mapping on the gateway.
    async fn remove_port(_: Arc<Self>, protocol: Protocol, port: u16)
        -> Result<(), Box<dyn Error>>;
}
