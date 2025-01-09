// Copyright 2022 Protocol Labs.
// Copyright 2018 Parity Technologies (UK) Ltd.
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
    convert::Infallible,
    task::{Context, Poll},
};

use libp2p_core::upgrade::PendingUpgrade;

use crate::handler::{
    ConnectionEvent, ConnectionHandler, ConnectionHandlerEvent, FullyNegotiatedInbound,
    FullyNegotiatedOutbound, SubstreamProtocol,
};

/// Implementation of [`ConnectionHandler`] that returns a pending upgrade.
#[derive(Clone, Debug)]
pub struct PendingConnectionHandler {
    protocol_name: String,
}

impl PendingConnectionHandler {
    pub fn new(protocol_name: String) -> Self {
        PendingConnectionHandler { protocol_name }
    }
}

impl ConnectionHandler for PendingConnectionHandler {
    type FromBehaviour = Infallible;
    type ToBehaviour = Infallible;
    type InboundProtocol = PendingUpgrade<String>;
    type OutboundProtocol = PendingUpgrade<String>;
    type OutboundOpenInfo = Infallible;
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(PendingUpgrade::new(self.protocol_name.clone()), ())
    }

    fn on_behaviour_event(&mut self, v: Self::FromBehaviour) {
        // TODO: remove when Rust 1.82 is MSRV
        #[allow(unreachable_patterns)]
        libp2p_core::util::unreachable(v)
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, Infallible, Self::ToBehaviour>> {
        Poll::Pending
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol, (), Infallible>,
    ) {
        match event {
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol, ..
            }) => libp2p_core::util::unreachable(protocol),
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol,
                info: _info,
            }) => {
                libp2p_core::util::unreachable(protocol);
                #[allow(unreachable_code, clippy::used_underscore_binding)]
                {
                    libp2p_core::util::unreachable(_info);
                }
            }
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            ConnectionEvent::AddressChange(_)
            | ConnectionEvent::DialUpgradeError(_)
            | ConnectionEvent::ListenUpgradeError(_)
            | ConnectionEvent::LocalProtocolsChange(_)
            | ConnectionEvent::RemoteProtocolsChange(_) => {}
        }
    }
}
