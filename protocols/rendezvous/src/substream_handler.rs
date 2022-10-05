// Copyright 2021 COMIT Network.
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

//! A generic [`ConnectionHandler`] that delegates the handling of substreams to [`SubstreamHandler`]s.
//!
//! This module is an attempt to simplify the implementation of protocols by freeing implementations from dealing with aspects such as concurrent substreams.
//! Particularly for outbound substreams, it greatly simplifies the definition of protocols through the [`FutureSubstream`] helper.
//!
//! At the moment, this module is an implementation detail of the rendezvous protocol but the intent is for it to be provided as a generic module that is accessible to other protocols as well.

use futures::future::{self, BoxFuture, Fuse, FusedFuture};
use futures::FutureExt;
use instant::Instant;
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_swarm::handler::{InboundUpgradeSend, OutboundUpgradeSend};
use libp2p_swarm::{
    ConnectionHandler, ConnectionHandlerEvent, ConnectionHandlerUpgrErr, KeepAlive,
    NegotiatedSubstream, SubstreamProtocol,
};
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::hash::Hash;
use std::task::{Context, Poll};
use std::time::Duration;
use void::Void;

/// Handles a substream throughout its lifetime.
pub trait SubstreamHandler: Sized {
    type InEvent;
    type OutEvent;
    type Error;
    type OpenInfo;

    fn upgrade(open_info: Self::OpenInfo)
        -> SubstreamProtocol<PassthroughProtocol, Self::OpenInfo>;
    fn new(substream: NegotiatedSubstream, info: Self::OpenInfo) -> Self;
    fn inject_event(self, event: Self::InEvent) -> Self;
    fn advance(self, cx: &mut Context<'_>) -> Result<Next<Self, Self::OutEvent>, Self::Error>;
}

/// The result of advancing a [`SubstreamHandler`].
pub enum Next<TState, TEvent> {
    /// Return the given event and set the handler into `next_state`.
    EmitEvent { event: TEvent, next_state: TState },
    /// The handler currently cannot do any more work, set its state back into `next_state`.
    Pending { next_state: TState },
    /// The handler performed some work and wants to continue in the given state.
    ///
    /// This variant is useful because it frees the handler from implementing a loop internally.
    Continue { next_state: TState },
    /// The handler finished.
    Done,
}

impl<TState, TEvent> Next<TState, TEvent> {
    pub fn map_state<TNextState>(
        self,
        map: impl FnOnce(TState) -> TNextState,
    ) -> Next<TNextState, TEvent> {
        match self {
            Next::EmitEvent { event, next_state } => Next::EmitEvent {
                event,
                next_state: map(next_state),
            },
            Next::Pending { next_state } => Next::Pending {
                next_state: map(next_state),
            },
            Next::Continue { next_state } => Next::Pending {
                next_state: map(next_state),
            },
            Next::Done => Next::Done,
        }
    }
}

#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
pub struct InboundSubstreamId(u64);

impl InboundSubstreamId {
    fn fetch_and_increment(&mut self) -> Self {
        let next_id = *self;
        self.0 += 1;

        next_id
    }
}

impl fmt::Display for InboundSubstreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
pub struct OutboundSubstreamId(u64);

impl OutboundSubstreamId {
    fn fetch_and_increment(&mut self) -> Self {
        let next_id = *self;
        self.0 += 1;

        next_id
    }
}

impl fmt::Display for OutboundSubstreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub struct PassthroughProtocol {
    ident: Option<&'static [u8]>,
}

impl PassthroughProtocol {
    pub fn new(ident: &'static [u8]) -> Self {
        Self { ident: Some(ident) }
    }
}

impl UpgradeInfo for PassthroughProtocol {
    type Info = &'static [u8];
    type InfoIter = std::option::IntoIter<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        self.ident.into_iter()
    }
}

impl<C: Send + 'static> InboundUpgrade<C> for PassthroughProtocol {
    type Output = C;
    type Error = Void;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: C, _: Self::Info) -> Self::Future {
        match self.ident {
            Some(_) => future::ready(Ok(socket)).boxed(),
            None => future::pending().boxed(),
        }
    }
}

impl<C: Send + 'static> OutboundUpgrade<C> for PassthroughProtocol {
    type Output = C;
    type Error = Void;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: C, _: Self::Info) -> Self::Future {
        match self.ident {
            Some(_) => future::ready(Ok(socket)).boxed(),
            None => future::pending().boxed(),
        }
    }
}

/// An implementation of [`ConnectionHandler`] that delegates to individual [`SubstreamHandler`]s.
pub struct SubstreamConnectionHandler<TInboundSubstream, TOutboundSubstream, TOutboundOpenInfo> {
    inbound_substreams: HashMap<InboundSubstreamId, TInboundSubstream>,
    outbound_substreams: HashMap<OutboundSubstreamId, TOutboundSubstream>,
    next_inbound_substream_id: InboundSubstreamId,
    next_outbound_substream_id: OutboundSubstreamId,

    new_substreams: VecDeque<TOutboundOpenInfo>,

    initial_keep_alive_deadline: Instant,
}

impl<TInboundSubstream, TOutboundSubstream, TOutboundOpenInfo>
    SubstreamConnectionHandler<TInboundSubstream, TOutboundSubstream, TOutboundOpenInfo>
{
    pub fn new(initial_keep_alive: Duration) -> Self {
        Self {
            inbound_substreams: Default::default(),
            outbound_substreams: Default::default(),
            next_inbound_substream_id: InboundSubstreamId(0),
            next_outbound_substream_id: OutboundSubstreamId(0),
            new_substreams: Default::default(),
            initial_keep_alive_deadline: Instant::now() + initial_keep_alive,
        }
    }
}

impl<TOutboundSubstream, TOutboundOpenInfo>
    SubstreamConnectionHandler<void::Void, TOutboundSubstream, TOutboundOpenInfo>
{
    pub fn new_outbound_only(initial_keep_alive: Duration) -> Self {
        Self {
            inbound_substreams: Default::default(),
            outbound_substreams: Default::default(),
            next_inbound_substream_id: InboundSubstreamId(0),
            next_outbound_substream_id: OutboundSubstreamId(0),
            new_substreams: Default::default(),
            initial_keep_alive_deadline: Instant::now() + initial_keep_alive,
        }
    }
}

impl<TInboundSubstream, TOutboundOpenInfo>
    SubstreamConnectionHandler<TInboundSubstream, void::Void, TOutboundOpenInfo>
{
    pub fn new_inbound_only(initial_keep_alive: Duration) -> Self {
        Self {
            inbound_substreams: Default::default(),
            outbound_substreams: Default::default(),
            next_inbound_substream_id: InboundSubstreamId(0),
            next_outbound_substream_id: OutboundSubstreamId(0),
            new_substreams: Default::default(),
            initial_keep_alive_deadline: Instant::now() + initial_keep_alive,
        }
    }
}

/// Poll all substreams within the given HashMap.
///
/// This is defined as a separate function because we call it with two different fields stored within [`SubstreamConnectionHandler`].
fn poll_substreams<TId, TSubstream, TError, TOutEvent>(
    substreams: &mut HashMap<TId, TSubstream>,
    cx: &mut Context<'_>,
) -> Poll<Result<(TId, TOutEvent), (TId, TError)>>
where
    TSubstream: SubstreamHandler<OutEvent = TOutEvent, Error = TError>,
    TId: Copy + Eq + Hash + fmt::Display,
{
    let substream_ids = substreams.keys().copied().collect::<Vec<_>>();

    'loop_substreams: for id in substream_ids {
        let mut handler = substreams
            .remove(&id)
            .expect("we just got the key out of the map");

        let (next_state, poll) = 'loop_handler: loop {
            match handler.advance(cx) {
                Ok(Next::EmitEvent { next_state, event }) => {
                    break (next_state, Poll::Ready(Ok((id, event))))
                }
                Ok(Next::Pending { next_state }) => break (next_state, Poll::Pending),
                Ok(Next::Continue { next_state }) => {
                    handler = next_state;
                    continue 'loop_handler;
                }
                Ok(Next::Done) => {
                    log::debug!("Substream handler {} finished", id);
                    continue 'loop_substreams;
                }
                Err(e) => return Poll::Ready(Err((id, e))),
            }
        };

        substreams.insert(id, next_state);

        return poll;
    }

    Poll::Pending
}

/// Event sent from the [`libp2p_swarm::NetworkBehaviour`] to the [`SubstreamConnectionHandler`].
#[allow(clippy::enum_variant_names)]
#[derive(Debug)]
pub enum InEvent<I, TInboundEvent, TOutboundEvent> {
    /// Open a new substream using the provided `open_info`.
    ///
    /// For "client-server" protocols, this is typically the initial message to be sent to the other party.
    NewSubstream { open_info: I },
    NotifyInboundSubstream {
        id: InboundSubstreamId,
        message: TInboundEvent,
    },
    NotifyOutboundSubstream {
        id: OutboundSubstreamId,
        message: TOutboundEvent,
    },
}

/// Event produced by the [`SubstreamConnectionHandler`] for the corresponding [`libp2p_swarm::NetworkBehaviour`].
#[derive(Debug)]
pub enum OutEvent<TInbound, TOutbound, TInboundError, TOutboundError> {
    /// An inbound substream produced an event.
    InboundEvent {
        id: InboundSubstreamId,
        message: TInbound,
    },
    /// An outbound substream produced an event.
    OutboundEvent {
        id: OutboundSubstreamId,
        message: TOutbound,
    },
    /// An inbound substream errored irrecoverably.
    InboundError {
        id: InboundSubstreamId,
        error: TInboundError,
    },
    /// An outbound substream errored irrecoverably.
    OutboundError {
        id: OutboundSubstreamId,
        error: TOutboundError,
    },
}

impl<
        TInboundInEvent,
        TInboundOutEvent,
        TOutboundInEvent,
        TOutboundOutEvent,
        TOutboundOpenInfo,
        TInboundError,
        TOutboundError,
        TInboundSubstreamHandler,
        TOutboundSubstreamHandler,
    > ConnectionHandler
    for SubstreamConnectionHandler<
        TInboundSubstreamHandler,
        TOutboundSubstreamHandler,
        TOutboundOpenInfo,
    >
where
    TInboundSubstreamHandler: SubstreamHandler<
        InEvent = TInboundInEvent,
        OutEvent = TInboundOutEvent,
        Error = TInboundError,
        OpenInfo = (),
    >,
    TOutboundSubstreamHandler: SubstreamHandler<
        InEvent = TOutboundInEvent,
        OutEvent = TOutboundOutEvent,
        Error = TOutboundError,
        OpenInfo = TOutboundOpenInfo,
    >,
    TInboundInEvent: fmt::Debug + Send + 'static,
    TInboundOutEvent: fmt::Debug + Send + 'static,
    TOutboundInEvent: fmt::Debug + Send + 'static,
    TOutboundOutEvent: fmt::Debug + Send + 'static,
    TOutboundOpenInfo: fmt::Debug + Send + 'static,
    TInboundError: fmt::Debug + Send + 'static,
    TOutboundError: fmt::Debug + Send + 'static,
    TInboundSubstreamHandler: Send + 'static,
    TOutboundSubstreamHandler: Send + 'static,
{
    type InEvent = InEvent<TOutboundOpenInfo, TInboundInEvent, TOutboundInEvent>;
    type OutEvent = OutEvent<TInboundOutEvent, TOutboundOutEvent, TInboundError, TOutboundError>;
    type Error = Void;
    type InboundProtocol = PassthroughProtocol;
    type OutboundProtocol = PassthroughProtocol;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = TOutboundOpenInfo;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        TInboundSubstreamHandler::upgrade(())
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgradeSend>::Output,
        _: Self::InboundOpenInfo,
    ) {
        self.inbound_substreams.insert(
            self.next_inbound_substream_id.fetch_and_increment(),
            TInboundSubstreamHandler::new(protocol, ()),
        );
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgradeSend>::Output,
        info: Self::OutboundOpenInfo,
    ) {
        self.outbound_substreams.insert(
            self.next_outbound_substream_id.fetch_and_increment(),
            TOutboundSubstreamHandler::new(protocol, info),
        );
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            InEvent::NewSubstream { open_info } => self.new_substreams.push_back(open_info),
            InEvent::NotifyInboundSubstream { id, message } => {
                match self.inbound_substreams.remove(&id) {
                    Some(handler) => {
                        let new_handler = handler.inject_event(message);

                        self.inbound_substreams.insert(id, new_handler);
                    }
                    None => {
                        log::debug!("Substream with ID {} not found", id);
                    }
                }
            }
            InEvent::NotifyOutboundSubstream { id, message } => {
                match self.outbound_substreams.remove(&id) {
                    Some(handler) => {
                        let new_handler = handler.inject_event(message);

                        self.outbound_substreams.insert(id, new_handler);
                    }
                    None => {
                        log::debug!("Substream with ID {} not found", id);
                    }
                }
            }
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        _: ConnectionHandlerUpgrErr<Void>,
    ) {
        // TODO: Handle upgrade errors properly
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        // Rudimentary keep-alive handling, to be extended as needed as this abstraction is used more by other protocols.

        if Instant::now() < self.initial_keep_alive_deadline {
            return KeepAlive::Yes;
        }

        if self.inbound_substreams.is_empty()
            && self.outbound_substreams.is_empty()
            && self.new_substreams.is_empty()
        {
            return KeepAlive::No;
        }

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
        if let Some(open_info) = self.new_substreams.pop_front() {
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: TOutboundSubstreamHandler::upgrade(open_info),
            });
        }

        match poll_substreams(&mut self.inbound_substreams, cx) {
            Poll::Ready(Ok((id, message))) => {
                return Poll::Ready(ConnectionHandlerEvent::Custom(OutEvent::InboundEvent {
                    id,
                    message,
                }))
            }
            Poll::Ready(Err((id, error))) => {
                return Poll::Ready(ConnectionHandlerEvent::Custom(OutEvent::InboundError {
                    id,
                    error,
                }))
            }
            Poll::Pending => {}
        }

        match poll_substreams(&mut self.outbound_substreams, cx) {
            Poll::Ready(Ok((id, message))) => {
                return Poll::Ready(ConnectionHandlerEvent::Custom(OutEvent::OutboundEvent {
                    id,
                    message,
                }))
            }
            Poll::Ready(Err((id, error))) => {
                return Poll::Ready(ConnectionHandlerEvent::Custom(OutEvent::OutboundError {
                    id,
                    error,
                }))
            }
            Poll::Pending => {}
        }

        Poll::Pending
    }
}

/// A helper struct for substream handlers that can be implemented as async functions.
///
/// This only works for substreams without an `InEvent` because - once constructed - the state of an inner future is opaque.
pub struct FutureSubstream<TOutEvent, TError> {
    future: Fuse<BoxFuture<'static, Result<TOutEvent, TError>>>,
}

impl<TOutEvent, TError> FutureSubstream<TOutEvent, TError> {
    pub fn new(future: impl Future<Output = Result<TOutEvent, TError>> + Send + 'static) -> Self {
        Self {
            future: future.boxed().fuse(),
        }
    }

    pub fn advance(mut self, cx: &mut Context<'_>) -> Result<Next<Self, TOutEvent>, TError> {
        if self.future.is_terminated() {
            return Ok(Next::Done);
        }

        match self.future.poll_unpin(cx) {
            Poll::Ready(Ok(event)) => Ok(Next::EmitEvent {
                event,
                next_state: self,
            }),
            Poll::Ready(Err(error)) => Err(error),
            Poll::Pending => Ok(Next::Pending { next_state: self }),
        }
    }
}

impl SubstreamHandler for void::Void {
    type InEvent = void::Void;
    type OutEvent = void::Void;
    type Error = void::Void;
    type OpenInfo = ();

    fn new(_: NegotiatedSubstream, _: Self::OpenInfo) -> Self {
        unreachable!("we should never yield a substream")
    }

    fn inject_event(self, event: Self::InEvent) -> Self {
        void::unreachable(event)
    }

    fn advance(self, _: &mut Context<'_>) -> Result<Next<Self, Self::OutEvent>, Self::Error> {
        void::unreachable(self)
    }

    fn upgrade(
        open_info: Self::OpenInfo,
    ) -> SubstreamProtocol<PassthroughProtocol, Self::OpenInfo> {
        SubstreamProtocol::new(PassthroughProtocol { ident: None }, open_info)
    }
}
