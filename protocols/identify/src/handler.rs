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

use crate::protocol::{
    InboundPush, Info, OutboundPush, Protocol, PushProtocol, ReplySubstream, UpgradeError,
};
use futures::future::BoxFuture;
use futures::prelude::*;
use futures_timer::Delay;
use libp2p_core::either::{EitherError, EitherOutput};
use libp2p_core::upgrade::{EitherUpgrade, InboundUpgrade, OutboundUpgrade, SelectUpgrade};
use libp2p_core::{ConnectedPoint, PeerId};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, IntoConnectionHandler,
    KeepAlive, NegotiatedSubstream, SubstreamProtocol,
};
use log::warn;
use smallvec::SmallVec;
use std::{io, pin::Pin, task::Context, task::Poll, time::Duration};

pub struct Proto {
    initial_delay: Duration,
    interval: Duration,
}

impl Proto {
    pub fn new(initial_delay: Duration, interval: Duration) -> Self {
        Proto {
            initial_delay,
            interval,
        }
    }
}

impl IntoConnectionHandler for Proto {
    type Handler = Handler;

    fn into_handler(self, remote_peer_id: &PeerId, _endpoint: &ConnectedPoint) -> Self::Handler {
        Handler::new(self.initial_delay, self.interval, *remote_peer_id)
    }

    fn inbound_protocol(&self) -> <Self::Handler as ConnectionHandler>::InboundProtocol {
        SelectUpgrade::new(Protocol, PushProtocol::inbound())
    }
}

/// Protocol handler for sending and receiving identification requests.
///
/// Outbound requests are sent periodically. The handler performs expects
/// at least one identification request to be answered by the remote before
/// permitting the underlying connection to be closed.
pub struct Handler {
    remote_peer_id: PeerId,
    inbound_identify_push: Option<BoxFuture<'static, Result<Info, UpgradeError>>>,
    /// Pending events to yield.
    events: SmallVec<
        [ConnectionHandlerEvent<
            EitherUpgrade<Protocol, PushProtocol<OutboundPush>>,
            (),
            Event,
            io::Error,
        >; 4],
    >,

    /// Future that fires when we need to identify the node again.
    trigger_next_identify: Delay,

    /// Whether the handler should keep the connection alive.
    keep_alive: KeepAlive,

    /// The interval of `trigger_next_identify`, i.e. the recurrent delay.
    interval: Duration,
}

/// Event produced by the `IdentifyHandler`.
#[derive(Debug)]
pub enum Event {
    /// We obtained identification information from the remote.
    Identified(Info),
    /// We actively pushed our identification information to the remote.
    IdentificationPushed,
    /// We received a request for identification.
    Identify(ReplySubstream<NegotiatedSubstream>),
    /// Failed to identify the remote.
    IdentificationError(ConnectionHandlerUpgrErr<UpgradeError>),
}

/// Identifying information of the local node that is pushed to a remote.
#[derive(Debug)]
pub struct Push(pub Info);

impl Handler {
    /// Creates a new `IdentifyHandler`.
    pub fn new(initial_delay: Duration, interval: Duration, remote_peer_id: PeerId) -> Self {
        Self {
            remote_peer_id,
            inbound_identify_push: Default::default(),
            events: SmallVec::new(),
            trigger_next_identify: Delay::new(initial_delay),
            keep_alive: KeepAlive::Yes,
            interval,
        }
    }
}

impl ConnectionHandler for Handler {
    type InEvent = Push;
    type OutEvent = Event;
    type Error = io::Error;
    type InboundProtocol = SelectUpgrade<Protocol, PushProtocol<InboundPush>>;
    type OutboundProtocol = EitherUpgrade<Protocol, PushProtocol<OutboundPush>>;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(SelectUpgrade::new(Protocol, PushProtocol::inbound()), ())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        output: <Self::InboundProtocol as InboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::InboundOpenInfo,
    ) {
        match output {
            EitherOutput::First(substream) => self
                .events
                .push(ConnectionHandlerEvent::Custom(Event::Identify(substream))),
            EitherOutput::Second(fut) => {
                if self.inbound_identify_push.replace(fut).is_some() {
                    warn!(
                        "New inbound identify push stream from {} while still \
                         upgrading previous one. Replacing previous with new.",
                        self.remote_peer_id,
                    );
                }
            }
        }
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        output: <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Output,
        _: Self::OutboundOpenInfo,
    ) {
        match output {
            EitherOutput::First(remote_info) => {
                self.events
                    .push(ConnectionHandlerEvent::Custom(Event::Identified(
                        remote_info,
                    )));
                self.keep_alive = KeepAlive::No;
            }
            EitherOutput::Second(()) => self
                .events
                .push(ConnectionHandlerEvent::Custom(Event::IdentificationPushed)),
        }
    }

    fn inject_event(&mut self, Push(push): Self::InEvent) {
        self.events
            .push(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(
                    EitherUpgrade::B(PushProtocol::outbound(push)),
                    (),
                ),
            });
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _info: Self::OutboundOpenInfo,
        err: ConnectionHandlerUpgrErr<
            <Self::OutboundProtocol as OutboundUpgrade<NegotiatedSubstream>>::Error,
        >,
    ) {
        use libp2p_core::upgrade::UpgradeError;

        let err = err.map_upgrade_err(|e| match e {
            UpgradeError::Select(e) => UpgradeError::Select(e),
            UpgradeError::Apply(EitherError::A(ioe)) => UpgradeError::Apply(ioe),
            UpgradeError::Apply(EitherError::B(ioe)) => UpgradeError::Apply(ioe),
        });
        self.events
            .push(ConnectionHandlerEvent::Custom(Event::IdentificationError(
                err,
            )));
        self.keep_alive = KeepAlive::No;
        self.trigger_next_identify.reset(self.interval);
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.keep_alive
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ConnectionHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Event, Self::Error>,
    > {
        if !self.events.is_empty() {
            return Poll::Ready(self.events.remove(0));
        }

        // Poll the future that fires when we need to identify the node again.
        match Future::poll(Pin::new(&mut self.trigger_next_identify), cx) {
            Poll::Pending => {}
            Poll::Ready(()) => {
                self.trigger_next_identify.reset(self.interval);
                let ev = ConnectionHandlerEvent::OutboundSubstreamRequest {
                    protocol: SubstreamProtocol::new(EitherUpgrade::A(Protocol), ()),
                };
                return Poll::Ready(ev);
            }
        }

        if let Some(Poll::Ready(res)) = self
            .inbound_identify_push
            .as_mut()
            .map(|f| f.poll_unpin(cx))
        {
            self.inbound_identify_push.take();

            if let Ok(info) = res {
                return Poll::Ready(ConnectionHandlerEvent::Custom(Event::Identified(info)));
            }
        }

        Poll::Pending
    }
}
