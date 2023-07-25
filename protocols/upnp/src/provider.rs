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

use std::{error::Error, fmt, net::Ipv4Addr};

use crate::{
    behaviour::{GatewayEvent, GatewayRequest},
    Config,
};
use async_trait::async_trait;
use futures::channel::mpsc::{Receiver, Sender};

/// Protocols available for port mapping.
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub(crate) enum Protocol {
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

/// Interface that interacts with the inner gateway by messages,
/// `GatewayRequest`s and `GatewayEvent`s.
pub struct Gateway {
    pub(crate) sender: Sender<GatewayRequest>,
    pub(crate) receiver: Receiver<GatewayEvent>,
    pub(crate) addr: Ipv4Addr,
}

/// Abstraction to allow for compatibility with various async runtimes.
#[async_trait]
pub trait Provider {
    async fn search_gateway(config: Config) -> Result<Gateway, Box<dyn Error>>;
}

macro_rules! impl_provider {
    ($impl:ident, $executor: ident, $gateway:ident, $protocol: ident, $options: ident) => {
        use super::{Config, Gateway, Protocol};
        use crate::behaviour::{GatewayEvent, GatewayRequest};

        use async_trait::async_trait;
        use futures::{channel::mpsc, SinkExt, StreamExt};
        use std::error::Error;

        impl From<Protocol> for $protocol {
            fn from(protocol: Protocol) -> Self {
                match protocol {
                    Protocol::Tcp => $protocol::TCP,
                    Protocol::Udp => $protocol::UDP,
                }
            }
        }

        #[async_trait]
        impl super::Provider for $impl {
            async fn search_gateway(config: Config) -> Result<super::Gateway, Box<dyn Error>> {
                let options = $options {
                    bind_addr: config.bind_addr,
                    broadcast_address: config.broadcast_addr,
                    timeout: config.timeout,
                };
                let gateway = $gateway::search_gateway(options).await?;
                let addr = gateway.get_external_ip().await?;

                let (events_sender, mut task_receiver) = mpsc::channel(10);
                let (mut task_sender, events_queue) = mpsc::channel(0);

                $executor::spawn(async move {
                    loop {
                        // The task sender has dropped so we can return.
                        let Some(req) = task_receiver.next().await else { return; };
                        let event = match req {
                            GatewayRequest::AddMapping { mapping, duration } => {
                                let duration = duration.unwrap_or(0);
                                let gateway = gateway.clone();
                                match gateway
                                    .add_port(
                                        mapping.protocol.into(),
                                        mapping.internal_addr.port(),
                                        mapping.internal_addr,
                                        duration,
                                        "rust-libp2p mapping",
                                    )
                                    .await
                                {
                                    Ok(()) => GatewayEvent::Mapped(mapping),
                                    Err(err) => GatewayEvent::MapFailure(mapping, err.into()),
                                }
                            }
                            GatewayRequest::RemoveMapping(mapping) => {
                                let gateway = gateway.clone();
                                match gateway
                                    .remove_port(
                                        mapping.protocol.into(),
                                        mapping.internal_addr.port(),
                                    )
                                    .await
                                {
                                    Ok(()) => GatewayEvent::Removed(mapping),
                                    Err(err) => GatewayEvent::RemovalFailure(mapping, err.into()),
                                }
                            }
                        };
                        task_sender
                            .send(event)
                            .await
                            .expect("receiver should be available");
                    }
                });

                Ok(Gateway {
                    sender: events_sender,
                    receiver: events_queue,
                    addr,
                })
            }
        }
    };
}

#[cfg(feature = "tokio")]
pub mod tokio {
    use igd::{aio, PortMappingProtocol, SearchOptions};

    #[doc(hidden)]
    pub struct Tokio;
    impl_provider! {Tokio, tokio, aio, PortMappingProtocol, SearchOptions}

    /// The type of a [`Behaviour`] using the `tokio` implementation.
    pub type Behaviour = crate::behaviour::Behaviour<Tokio>;
}

#[cfg(feature = "async-std")]
pub mod async_std {
    use async_std::task;
    use igd_async_std::{aio, PortMappingProtocol, SearchOptions};

    #[doc(hidden)]
    pub struct AsyncStd;
    impl_provider! {AsyncStd, task, aio, PortMappingProtocol, SearchOptions}

    /// The type of a [`Behaviour`] using the `async-std` implementation.
    pub type Behaviour = crate::behaviour::Behaviour<AsyncStd>;
}
