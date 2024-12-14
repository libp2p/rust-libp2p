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
    convert::Infallible,
    task::{Context, Poll},
};

use futures::FutureExt;
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_swarm::{
    handler::{
        ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
        ListenUpgradeError,
    },
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol,
};
use tracing::error;

use crate::Run;

#[derive(Debug)]
pub struct Event {
    pub stats: Run,
}

pub struct Handler {
    inbound: futures_bounded::FuturesSet<Result<Run, std::io::Error>>,
}

impl Handler {
    pub fn new() -> Self {
        Self {
            inbound: futures_bounded::FuturesSet::new(
                crate::RUN_TIMEOUT,
                crate::MAX_PARALLEL_RUNS_PER_CONNECTION,
            ),
        }
    }
}

impl Default for Handler {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Infallible;
    type ToBehaviour = Event;
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundProtocol = DeniedUpgrade;
    type OutboundOpenInfo = Infallible;
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(ReadyUpgrade::new(crate::PROTOCOL_NAME), ())
    }

    fn on_behaviour_event(&mut self, v: Self::FromBehaviour) {
        // TODO: remove when Rust 1.82 is MSRV
        #[allow(unreachable_patterns)]
        libp2p_core::util::unreachable(v)
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol, (), Infallible>,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol,
                info: _,
            }) => {
                if self
                    .inbound
                    .try_push(crate::protocol::receive_send(protocol).boxed())
                    .is_err()
                {
                    tracing::warn!("Dropping inbound stream because we are at capacity");
                }
            }
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound { info, .. }) => {
                libp2p_core::util::unreachable(info)
            }

            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            ConnectionEvent::DialUpgradeError(DialUpgradeError { info, .. }) => {
                libp2p_core::util::unreachable(info)
            }
            ConnectionEvent::AddressChange(_)
            | ConnectionEvent::LocalProtocolsChange(_)
            | ConnectionEvent::RemoteProtocolsChange(_) => {}
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            ConnectionEvent::ListenUpgradeError(ListenUpgradeError { info: (), error }) => {
                libp2p_core::util::unreachable(error)
            }
            _ => {}
        }
    }

    #[tracing::instrument(level = "trace", name = "ConnectionHandler::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, Infallible, Self::ToBehaviour>> {
        loop {
            match self.inbound.poll_unpin(cx) {
                Poll::Ready(Ok(Ok(stats))) => {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event { stats }))
                }
                Poll::Ready(Ok(Err(e))) => {
                    error!("{e:?}");
                    continue;
                }
                Poll::Ready(Err(e @ futures_bounded::Timeout { .. })) => {
                    error!("inbound perf request timed out: {e}");
                    continue;
                }
                Poll::Pending => {}
            }

            return Poll::Pending;
        }
    }
}
