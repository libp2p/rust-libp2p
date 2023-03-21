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
    task::{Context, Poll},
    time::{Duration, Instant},
};

use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_swarm::{
    handler::{
        ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
        ListenUpgradeError,
    },
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, KeepAlive,
    SubstreamProtocol,
};
use log::error;
use void::Void;

use super::RunStats;

#[derive(Debug)]
pub struct Event {
    pub stats: RunStats,
}

pub struct Handler {
    inbound: FuturesUnordered<BoxFuture<'static, Result<RunStats, std::io::Error>>>,
    keep_alive: KeepAlive,
}

impl Handler {
    pub fn new() -> Self {
        Self {
            inbound: Default::default(),
            keep_alive: KeepAlive::Yes,
        }
    }
}

impl Default for Handler {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionHandler for Handler {
    type InEvent = Void;
    type OutEvent = Event;
    type Error = Void;
    type InboundProtocol = ReadyUpgrade<&'static [u8]>;
    type OutboundProtocol = DeniedUpgrade;
    type OutboundOpenInfo = Void;
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(crate::PROTOCOL_NAME), ())
    }

    fn on_behaviour_event(&mut self, v: Self::InEvent) {
        void::unreachable(v)
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol,
                info: _,
            }) => {
                self.inbound
                    .push(crate::protocol::receive_send(protocol).boxed());
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound { info, .. }) => {
                void::unreachable(info)
            }

            ConnectionEvent::DialUpgradeError(DialUpgradeError { info, .. }) => {
                void::unreachable(info)
            }
            ConnectionEvent::AddressChange(_) => {}
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError { info: (), error }) => {
                match error {
                    ConnectionHandlerUpgrErr::Timeout => {}
                    ConnectionHandlerUpgrErr::Timer => {}
                    ConnectionHandlerUpgrErr::Upgrade(error) => match error {
                        libp2p_core::UpgradeError::Select(_) => {}
                        libp2p_core::UpgradeError::Apply(v) => void::unreachable(v),
                    },
                }
            }
        }
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        while let Poll::Ready(Some(result)) = self.inbound.poll_next_unpin(cx) {
            match result {
                Ok(stats) => return Poll::Ready(ConnectionHandlerEvent::Custom(Event { stats })),
                Err(e) => {
                    error!("{e:?}")
                }
            }
        }

        if self.inbound.is_empty() {
            match self.keep_alive {
                KeepAlive::Yes => {
                    self.keep_alive = KeepAlive::Until(Instant::now() + Duration::from_secs(10));
                }
                KeepAlive::Until(_) => {}
                KeepAlive::No => panic!("Handler never sets KeepAlive::No."),
            }
        } else {
            self.keep_alive = KeepAlive::Yes
        }

        Poll::Pending
    }
}
