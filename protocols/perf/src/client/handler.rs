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
    time::{Duration, Instant},
};

use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt, TryFutureExt};
use libp2p_core::upgrade::{DeniedUpgrade, ReadyUpgrade};
use libp2p_swarm::{
    handler::{
        ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound,
        ListenUpgradeError,
    },
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, KeepAlive,
    SubstreamProtocol,
};
use void::Void;

use super::{RunId, RunParams, RunStats};

#[derive(Debug)]
pub struct Command {
    pub(crate) id: RunId,
    pub(crate) params: RunParams,
}

#[derive(Debug)]
pub struct Event {
    pub(crate) id: RunId,
    pub(crate) result: Result<RunStats, ConnectionHandlerUpgrErr<Void>>,
}

pub struct Handler {
    /// Queue of events to return when polled.
    queued_events: VecDeque<
        ConnectionHandlerEvent<
            <Self as ConnectionHandler>::OutboundProtocol,
            <Self as ConnectionHandler>::OutboundOpenInfo,
            <Self as ConnectionHandler>::OutEvent,
            <Self as ConnectionHandler>::Error,
        >,
    >,

    outbound: FuturesUnordered<BoxFuture<'static, Result<Event, std::io::Error>>>,

    keep_alive: KeepAlive,
}

impl Handler {
    pub fn new() -> Self {
        Self {
            queued_events: Default::default(),
            outbound: Default::default(),
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
    type InEvent = Command;
    type OutEvent = Event;
    type Error = Void;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = ReadyUpgrade<&'static [u8]>;
    type OutboundOpenInfo = Command;
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(DeniedUpgrade, ())
    }

    fn on_behaviour_event(&mut self, command: Self::InEvent) {
        self.queued_events
            .push_back(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(ReadyUpgrade::new(crate::PROTOCOL_NAME), command),
            })
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
                protocol, ..
            }) => void::unreachable(protocol),
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol,
                info: Command { params, id },
            }) => self.outbound.push(
                crate::protocol::send_receive(params, protocol)
                    .map_ok(move |timers| Event {
                        id,
                        result: Ok(RunStats { params, timers }),
                    })
                    .boxed(),
            ),

            ConnectionEvent::AddressChange(_) => {}
            ConnectionEvent::DialUpgradeError(DialUpgradeError {
                info: Command { id, .. },
                error,
            }) => self
                .queued_events
                .push_back(ConnectionHandlerEvent::Custom(Event {
                    id,
                    result: Err(error),
                })),
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
        // Return queued events.
        if let Some(event) = self.queued_events.pop_front() {
            return Poll::Ready(event);
        }

        while let Poll::Ready(Some(result)) = self.outbound.poll_next_unpin(cx) {
            match result {
                Ok(event) => return Poll::Ready(ConnectionHandlerEvent::Custom(event)),
                Err(e) => {
                    panic!("{e:?}")
                }
            }
        }

        if self.outbound.is_empty() {
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
