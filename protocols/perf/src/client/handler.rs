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
    collections::VecDeque,
    task::{Context, Poll},
};

use futures::{
    stream::{BoxStream, SelectAll},
    StreamExt,
};
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_swarm::{
    handler::{
        ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
        ListenUpgradeError,
    },
    ConnectionHandler, ConnectionHandlerEvent, StreamProtocol, SubstreamProtocol,
};

use crate::{
    client::{RunError, RunId},
    RunParams, RunUpdate,
};

#[derive(Debug)]
pub struct Command {
    pub(crate) id: RunId,
    pub(crate) params: RunParams,
}

#[derive(Debug)]
pub struct Event {
    pub(crate) id: RunId,
    pub(crate) result: Result<RunUpdate, RunError>,
}

pub struct Handler {
    /// Queue of events to return when polled.
    queued_events: VecDeque<
        ConnectionHandlerEvent<
            <Self as ConnectionHandler>::OutboundProtocol,
            (),
            <Self as ConnectionHandler>::ToBehaviour,
        >,
    >,

    requested_streams: VecDeque<Command>,

    outbound: SelectAll<BoxStream<'static, (RunId, Result<crate::RunUpdate, std::io::Error>)>>,
}

impl Handler {
    pub fn new() -> Self {
        Self {
            queued_events: Default::default(),
            requested_streams: Default::default(),
            outbound: Default::default(),
        }
    }
}

impl Default for Handler {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Command;
    type ToBehaviour = Event;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn on_behaviour_event(&mut self, command: Self::FromBehaviour) {
        self.requested_streams.push_back(command);
        self.queued_events
            .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(ReadyUpgrade::new(crate::PROTOCOL_NAME), ()),
            })
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
        match event {
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol, ..
            }) => libp2p_core::util::unreachable(protocol),
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol,
                info: (),
            }) => {
                let Command { id, params } = self
                    .requested_streams
                    .pop_front()
                    .expect("opened a stream without a pending command");
                self.outbound.push(
                    crate::protocol::send_receive(params, protocol)
                        .map(move |result| (id, result))
                        .boxed(),
                );
            }

            ConnectionEvent::AddressChange(_)
            | ConnectionEvent::LocalProtocolsChange(_)
            | ConnectionEvent::RemoteProtocolsChange(_) => {}
            ConnectionEvent::DialUpgradeError(DialUpgradeError { info: (), error }) => {
                let Command { id, .. } = self
                    .requested_streams
                    .pop_front()
                    .expect("requested stream without pending command");
                self.queued_events
                    .push_back(ConnectionHandlerEvent::NotifyBehaviour(Event {
                        id,
                        result: Err(error.into()),
                    }));
            }
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
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, (), Self::ToBehaviour>> {
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        if let Poll::Ready(Some((id, result))) = self.outbound.poll_next_unpin(cx) {
            return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Event {
                id,
                result: result.map_err(Into::into),
            }));
        }

        Poll::Pending
    }
}
