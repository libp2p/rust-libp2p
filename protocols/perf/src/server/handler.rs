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
    handler::{ConnectionEvent, FullyNegotiatedInbound},
    ConnectionHandler, ConnectionHandlerEvent, KeepAlive, SubstreamProtocol,
};
use void::Void;

#[derive(Debug)]
pub enum Event {}

#[derive(Default)]
pub struct Handler {
    inbound: FuturesUnordered<BoxFuture<'static, Result<(), std::io::Error>>>,
}

impl ConnectionHandler for Handler {
    type InEvent = ();
    type OutEvent = Event;
    type Error = std::io::Error;
    type InboundProtocol = ReadyUpgrade<&'static [u8]>;
    type OutboundProtocol = DeniedUpgrade;
    type OutboundOpenInfo = Void;
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(ReadyUpgrade::new(crate::PROTOCOL_NAME), ())
    }

    fn on_behaviour_event(&mut self, _: Self::InEvent) {}

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
            ConnectionEvent::FullyNegotiatedOutbound(_) => todo!(),
            ConnectionEvent::AddressChange(_) => todo!(),
            ConnectionEvent::DialUpgradeError(_) => todo!(),
            ConnectionEvent::ListenUpgradeError(_) => todo!(),
        }
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        // TODO
        KeepAlive::Yes
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
                Ok(()) => {}
                Err(e) => {
                    panic!("{e:?}")
                }
            }
        }

        Poll::Pending
    }
}
