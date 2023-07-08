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
    net::{Ipv4Addr, SocketAddrV4},
    sync::Arc,
    time::Duration,
};

use super::Protocol;
use crate::Config;

use async_trait::async_trait;
use futures::Future;
use igd_async_std::{
    aio::{self, Gateway},
    PortMappingProtocol, SearchOptions,
};

#[async_trait]
impl super::Gateway for Gateway {
    async fn search(config: Config) -> Result<(Self, Ipv4Addr), Box<dyn Error>> {
        let options = SearchOptions {
            bind_addr: config.bind_addr,
            broadcast_address: config.broadcast_addr,
            timeout: config.timeout,
        };
        let gateway = aio::search_gateway(options).await?;
        let addr = gateway.get_external_ip().await?;
        Ok((gateway, addr))
    }

    async fn add_port(
        gateway: Arc<Self>,
        protocol: Protocol,
        addr: SocketAddrV4,
        duration: Duration,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let protocol = match protocol {
            Protocol::Tcp => PortMappingProtocol::TCP,
            Protocol::Udp => PortMappingProtocol::UDP,
        };
        gateway
            .add_port(
                protocol,
                addr.port(),
                addr,
                duration.as_secs() as u32,
                "rust-libp2p mapping",
            )
            .await
            .map_err(|err| err.into())
    }

    async fn remove_port(
        gateway: Arc<Self>,
        protocol: Protocol,
        port: u16,
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        let protocol = match protocol {
            Protocol::Tcp => PortMappingProtocol::TCP,
            Protocol::Udp => PortMappingProtocol::UDP,
        };
        gateway
            .remove_port(protocol, port)
            .await
            .map_err(|err| err.into())
    }
    fn spawn<F>(f: F)
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        async_std::task::spawn(f);
    }
}

/// The type of a [`Behaviour`] using the `async-io` implementation.
pub type Behaviour = crate::behaviour::Behaviour<Gateway>;
