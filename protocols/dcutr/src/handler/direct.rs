// Copyright 2021 Protocol Labs.
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

//! [`ConnectionHandler`] handling direct connection upgraded through a relayed connection.

use libp2p_core::connection::ConnectionId;
use libp2p_core::upgrade::{DeniedUpgrade, InboundUpgrade, OutboundUpgrade};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, KeepAlive,
    NegotiatedSubstream, SubstreamProtocol,
};
use std::task::{Context, Poll};
use void::Void;

#[derive(Debug)]
pub enum Event {
    DirectConnectionUpgradeSucceeded { relayed_connection_id: ConnectionId },
}

pub struct Handler {
    relayed_connection_id: ConnectionId,
    reported: bool,
}

impl Handler {
    pub(crate) fn new(relayed_connection_id: ConnectionId) -> Self {
        Self {
            reported: false,
            relayed_connection_id,
        }
    }
}

impl ConnectionHandler for Handler {
    type InEvent = void::Void;
    type OutEvent = Event;
    type Error = ConnectionHandlerUpgrErr<std::io::Error>;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type OutboundOpenInfo = Void;
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        _: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::InboundOpenInfo,
    ) {
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        _: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::OutboundOpenInfo,
    ) {
    }

    fn inject_event(&mut self, _: Self::InEvent) {}

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        _: ConnectionHandlerUpgrErr<
            <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Error,
        >,
    ) {
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        KeepAlive::No
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        if !self.reported {
            self.reported = true;
            return Poll::Ready(ConnectionHandlerEvent::Custom(
                Event::DirectConnectionUpgradeSucceeded {
                    relayed_connection_id: self.relayed_connection_id,
                },
            ));
        }
        Poll::Pending
    }
}
