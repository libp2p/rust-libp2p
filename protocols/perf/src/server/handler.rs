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

use std::task::{Context, Poll};

use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_swarm::{
    handler::{
        ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
        ListenUpgradeError,
    },
    ConnectionHandler, ConnectionHandlerEvent, KeepAlive, StreamProtocol, SubstreamProtocol,
};
use log::error;
use void::Void;

use crate::Run;

#[derive(Debug)]
pub struct Event {
    pub stats: Run,
}

pub struct Handler {
    inbound: FuturesUnordered<BoxFuture<'static, Result<Run, std::io::Error>>>,
}

impl Handler {
    pub fn new() -> Self {
        Self {
            inbound: Default::default(),
        }
    }
}

impl Default for Handler {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Void;
    type ToBehaviour = Event;
    type Error = Void;
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundProtocol = DeniedUpgrade;
    type OutboundOpenInfo = Void;
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(crate::PROTOCOL_NAME), ())
    }

    fn on_behaviour_event(&mut self, v: Self::FromBehaviour) {
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
            ConnectionEvent::AddressChange(_)
            | ConnectionEvent::LocalProtocolsChange(_)
            | ConnectionEvent::RemoteProtocolsChange(_) => {}
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError { info: (), error }) => {
                void::unreachable(error)
            }
        }
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        if self.inbound.is_empty() {
            KeepAlive::No
        } else {
            KeepAlive::Yes
        }
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
            Self::Error,
        >,
    > {
        while let Poll::Ready(Some(result)) = self.inbound.poll_next_unpin(cx) {
            match result {
                Ok(stats) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event { stats }))
                }
                Err(e) => {
                    error!("{e:?}")
                }
            }
        }

        Poll::Pending
    }
}
