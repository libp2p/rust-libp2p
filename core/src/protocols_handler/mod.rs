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

use crate::{
    either::EitherOutput,
    nodes::handled_node::{NodeHandler, NodeHandlerEndpoint, NodeHandlerEvent},
    upgrade::{
        self,
        InboundUpgrade,
        InboundUpgradeExt,
        OutboundUpgrade,
        InboundUpgradeApply,
        OutboundUpgradeApply,
        DeniedUpgrade,
        EitherUpgrade,
        OrUpgrade
    }
};
use futures::prelude::*;
use std::{io, marker::PhantomData, time::Duration};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Timeout;
use void::Void;

/// Handler for a set of protocols for a specific connection with a remote.
///
/// This trait should be implemented on a struct that holds the state for a specific protocol
/// behaviour with a specific remote.
///
/// # Handling a protocol
///
/// Communication with a remote over a set of protocols opened in two different ways:
///
/// - Dialing, which is a voluntary process. In order to do so, make `poll()` return an
///   `OutboundSubstreamRequest` variant containing the connection upgrade to use to start using a protocol.
/// - Listening, which is used to determine which protocols are supported when the remote wants
///   to open a substream. The `listen_protocol()` method should return the upgrades supported when
///   listening.
///
/// The upgrade when dialing and the upgrade when listening have to be of the same type, but you
/// are free to return for example an `OrUpgrade` enum, or an enum of your own, containing the upgrade
/// you want depending on the situation.
///
/// # Shutting down
///
/// Implementors of this trait should keep in mind that the connection can be closed at any time.
/// When a connection is closed (either by us or by the remote) `shutdown()` is called and the
/// handler continues to be processed until it produces `None`. Only then the handler is destroyed.
///
/// This makes it possible for the handler to finish delivering events even after knowing that it
/// is shutting down.
///
/// Implementors of this trait should keep in mind that when `shutdown()` is called, the connection
/// might already be closed or unresponsive. They should therefore not rely on being able to
/// deliver messages.
///
/// # Relationship with `NodeHandler`.
///
/// This trait is very similar to the `NodeHandler` trait. The fundamental differences are:
///
/// - The `NodeHandler` trait gives you more control and is therefore more difficult to implement.
/// - The `NodeHandler` trait is designed to have exclusive ownership of the connection to a
///   node, while the `ProtocolsHandler` trait is designed to handle only a specific set of
///   protocols. Two or more implementations of `ProtocolsHandler` can be combined into one that
///   supports all the protocols together, which is not possible with `NodeHandler`.
///
// TODO: add a "blocks connection closing" system, so that we can gracefully close a connection
//       when it's no longer needed, and so that for example the periodic pinging system does not
//       keep the connection alive forever
pub trait ProtocolsHandler {
    /// Custom event that can be received from the outside.
    type InEvent;
    /// Custom event that can be produced by the handler and that will be returned to the outside.
    type OutEvent;
    /// The type of the substream that contains the raw data.
    type Substream: AsyncRead + AsyncWrite;
    /// The upgrade for the protocol or protocols handled by this handler.
    type InboundProtocol: InboundUpgrade<Self::Substream>;
    /// The upgrade for the protocol or protocols handled by this handler.
    type OutboundProtocol: OutboundUpgrade<Self::Substream>;
    /// Information about a substream. Can be sent to the handler through a `NodeHandlerEndpoint`,
    /// and will be passed back in `inject_substream` or `inject_outbound_closed`.
    type OutboundOpenInfo;

    /// Produces a `ConnectionUpgrade` for the protocol or protocols to accept when listening.
    ///
    /// > **Note**: You should always accept all the protocols you support, even if in a specific
    /// >           context you wouldn't accept one in particular (eg. only allow one substream at
    /// >           a time for a given protocol). The reason is that remotes are allowed to put the
    /// >           list of supported protocols in a cache in order to avoid spurious queries.
    fn listen_protocol(&self) -> Self::InboundProtocol;

    /// Injects a fully-negotiated substream in the handler.
    ///
    /// This method is called when a substream has been successfully opened and negotiated.
    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<Self::Substream>>::Output
    );

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Output,
        info: Self::OutboundOpenInfo
    );

    /// Injects an event coming from the outside in the handler.
    fn inject_event(&mut self, event: Self::InEvent);

    /// Indicates to the handler that upgrading a substream to the given protocol has failed.
    fn inject_dial_upgrade_error(&mut self, info: Self::OutboundOpenInfo, error: io::Error);

    /// Indicates to the handler that the inbound part of the muxer has been closed, and that
    /// therefore no more inbound substreams will be produced.
    fn inject_inbound_closed(&mut self);

    /// Indicates to the node that it should shut down. After that, it is expected that `poll()`
    /// returns `Ready(None)` as soon as possible.
    ///
    /// This method allows an implementation to perform a graceful shutdown of the substreams, and
    /// send back various events.
    fn shutdown(&mut self);

    /// Should behave like `Stream::poll()`. Should close if no more event can be produced and the
    /// node should be closed.
    ///
    /// > **Note**: If this handler is combined with other handlers, as soon as `poll()` returns
    /// >           `Ok(Async::Ready(None))`, all the other handlers will receive a call to
    /// >           `shutdown()` and will eventually be closed and destroyed.
    fn poll(&mut self) -> Poll<Option<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>>, io::Error>;

    /// Adds a closure that turns the input event into something else.
    #[inline]
    fn map_in_event<TNewIn, TMap>(self, map: TMap) -> MapInEvent<Self, TNewIn, TMap>
    where
        Self: Sized,
        TMap: Fn(&TNewIn) -> Option<&Self::InEvent>,
    {
        MapInEvent {
            inner: self,
            map,
            marker: PhantomData,
        }
    }

    /// Adds a closure that turns the output event into something else.
    #[inline]
    fn map_out_event<TMap, TNewOut>(self, map: TMap) -> MapOutEvent<Self, TMap>
    where
        Self: Sized,
        TMap: FnMut(Self::OutEvent) -> TNewOut,
    {
        MapOutEvent { inner: self, map }
    }

    /// Builds an implementation of `ProtocolsHandler` that handles both this protocol and the
    /// other one together.
    #[inline]
    fn select<TProto2>(self, other: TProto2) -> ProtocolsHandlerSelect<Self, TProto2>
    where
        Self: Sized,
    {
        ProtocolsHandlerSelect {
            proto1: self,
            proto2: other,
        }
    }

    /// Creates a builder that will allow creating a `NodeHandler` that handles this protocol
    /// exclusively.
    #[inline]
    fn into_node_handler_builder(self) -> NodeHandlerWrapperBuilder<Self>
    where
        Self: Sized,
    {
        NodeHandlerWrapperBuilder {
            handler: self,
            in_timeout: Duration::from_secs(10),
            out_timeout: Duration::from_secs(10),
        }
    }

    /// Builds an implementation of `NodeHandler` that handles this protocol exclusively.
    ///
    /// > **Note**: This is a shortcut for `self.into_node_handler_builder().build()`.
    #[inline]
    fn into_node_handler(self) -> NodeHandlerWrapper<Self>
    where
        Self: Sized,
    {
        self.into_node_handler_builder().build()
    }
}

/// Event produced by a handler.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProtocolsHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, TCustom> {
    /// Request a new outbound substream to be opened with the remote.
    OutboundSubstreamRequest {
        /// The upgrade to apply on the substream.
        upgrade: TConnectionUpgrade,
        /// User-defined information, passed back when the substream is open.
        info: TOutboundOpenInfo,
    },

    /// Other event.
    Custom(TCustom),
}

/// Event produced by a handler.
impl<TConnectionUpgrade, TOutboundOpenInfo, TCustom>
    ProtocolsHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, TCustom>
{
    /// If this is an `OutboundSubstreamRequest`, maps the `info` member from a `TOutboundOpenInfo` to something else.
    #[inline]
    pub fn map_outbound_open_info<F, I>(
        self,
        map: F,
    ) -> ProtocolsHandlerEvent<TConnectionUpgrade, I, TCustom>
    where
        F: FnOnce(TOutboundOpenInfo) -> I,
    {
        match self {
            ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    upgrade,
                    info: map(info),
                }
            }
            ProtocolsHandlerEvent::Custom(val) => ProtocolsHandlerEvent::Custom(val),
        }
    }

    /// If this is an `OutboundSubstreamRequest`, maps the protocol (`TConnectionUpgrade`) to something else.
    #[inline]
    pub fn map_protocol<F, I>(
        self,
        map: F,
    ) -> ProtocolsHandlerEvent<I, TOutboundOpenInfo, TCustom>
    where
        F: FnOnce(TConnectionUpgrade) -> I,
    {
        match self {
            ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    upgrade: map(upgrade),
                    info,
                }
            }
            ProtocolsHandlerEvent::Custom(val) => ProtocolsHandlerEvent::Custom(val),
        }
    }

    /// If this is a `Custom` event, maps the content to something else.
    #[inline]
    pub fn map_custom<F, I>(
        self,
        map: F,
    ) -> ProtocolsHandlerEvent<TConnectionUpgrade, TOutboundOpenInfo, I>
    where
        F: FnOnce(TCustom) -> I,
    {
        match self {
            ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info } => {
                ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info }
            }
            ProtocolsHandlerEvent::Custom(val) => ProtocolsHandlerEvent::Custom(map(val)),
        }
    }
}

/// Implementation of `ProtocolsHandler` that doesn't handle anything.
pub struct DummyProtocolsHandler<TSubstream> {
    shutting_down: bool,
    marker: PhantomData<TSubstream>,
}

impl<TSubstream> Default for DummyProtocolsHandler<TSubstream> {
    #[inline]
    fn default() -> Self {
        DummyProtocolsHandler {
            shutting_down: false,
            marker: PhantomData,
        }
    }
}

impl<TSubstream> ProtocolsHandler for DummyProtocolsHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type InEvent = Void;
    type OutEvent = Void;
    type Substream = TSubstream;
    type InboundProtocol = DeniedUpgrade;
    type OutboundProtocol = DeniedUpgrade;
    type OutboundOpenInfo = Void;

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        DeniedUpgrade
    }

    #[inline]
    fn inject_fully_negotiated_inbound(
        &mut self,
        _: <Self::InboundProtocol as InboundUpgrade<TSubstream>>::Output
    ) {
    }

    #[inline]
    fn inject_fully_negotiated_outbound(
        &mut self,
        _: <Self::OutboundProtocol as OutboundUpgrade<TSubstream>>::Output,
        _: Self::OutboundOpenInfo
    ) {
    }

    #[inline]
    fn inject_event(&mut self, _: Self::InEvent) {}

    #[inline]
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: io::Error) {}

    #[inline]
    fn inject_inbound_closed(&mut self) {}

    #[inline]
    fn shutdown(&mut self) {
        self.shutting_down = true;
    }

    #[inline]
    fn poll(
        &mut self,
    ) -> Poll<
        Option<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>>,
        io::Error,
    > {
        if self.shutting_down {
            Ok(Async::Ready(None))
        } else {
            Ok(Async::NotReady)
        }
    }
}

/// Wrapper around a protocol handler that turns the input event into something else.
pub struct MapInEvent<TProtoHandler, TNewIn, TMap> {
    inner: TProtoHandler,
    map: TMap,
    marker: PhantomData<TNewIn>,
}

impl<TProtoHandler, TMap, TNewIn> ProtocolsHandler for MapInEvent<TProtoHandler, TNewIn, TMap>
where
    TProtoHandler: ProtocolsHandler,
    TMap: Fn(TNewIn) -> Option<TProtoHandler::InEvent>,
{
    type InEvent = TNewIn;
    type OutEvent = TProtoHandler::OutEvent;
    type Substream = TProtoHandler::Substream;
    type InboundProtocol = TProtoHandler::InboundProtocol;
    type OutboundProtocol = TProtoHandler::OutboundProtocol;
    type OutboundOpenInfo = TProtoHandler::OutboundOpenInfo;

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        self.inner.listen_protocol()
    }

    #[inline]
    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<Self::Substream>>::Output
    ) {
        self.inner.inject_fully_negotiated_inbound(protocol)
    }

    #[inline]
    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Output,
        info: Self::OutboundOpenInfo
    ) {
        self.inner.inject_fully_negotiated_outbound(protocol, info)
    }

    #[inline]
    fn inject_event(&mut self, event: TNewIn) {
        if let Some(event) = (self.map)(event) {
            self.inner.inject_event(event);
        }
    }

    #[inline]
    fn inject_dial_upgrade_error(&mut self, info: Self::OutboundOpenInfo, error: io::Error) {
        self.inner.inject_dial_upgrade_error(info, error)
    }

    #[inline]
    fn inject_inbound_closed(&mut self) {
        self.inner.inject_inbound_closed()
    }

    #[inline]
    fn shutdown(&mut self) {
        self.inner.shutdown()
    }

    #[inline]
    fn poll(
        &mut self,
    ) -> Poll<
        Option<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>>,
        io::Error,
    > {
        self.inner.poll()
    }
}

/// Wrapper around a protocol handler that turns the output event into something else.
pub struct MapOutEvent<TProtoHandler, TMap> {
    inner: TProtoHandler,
    map: TMap,
}

impl<TProtoHandler, TMap, TNewOut> ProtocolsHandler for MapOutEvent<TProtoHandler, TMap>
where
    TProtoHandler: ProtocolsHandler,
    TMap: FnMut(TProtoHandler::OutEvent) -> TNewOut,
{
    type InEvent = TProtoHandler::InEvent;
    type OutEvent = TNewOut;
    type Substream = TProtoHandler::Substream;
    type InboundProtocol = TProtoHandler::InboundProtocol;
    type OutboundProtocol = TProtoHandler::OutboundProtocol;
    type OutboundOpenInfo = TProtoHandler::OutboundOpenInfo;

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        self.inner.listen_protocol()
    }

    #[inline]
    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgrade<Self::Substream>>::Output
    ) {
        self.inner.inject_fully_negotiated_inbound(protocol)
    }

    #[inline]
    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgrade<Self::Substream>>::Output,
        info: Self::OutboundOpenInfo
    ) {
        self.inner.inject_fully_negotiated_outbound(protocol, info)
    }

    #[inline]
    fn inject_event(&mut self, event: Self::InEvent) {
        self.inner.inject_event(event)
    }

    #[inline]
    fn inject_dial_upgrade_error(&mut self, info: Self::OutboundOpenInfo, error: io::Error) {
        self.inner.inject_dial_upgrade_error(info, error)
    }

    #[inline]
    fn inject_inbound_closed(&mut self) {
        self.inner.inject_inbound_closed()
    }

    #[inline]
    fn shutdown(&mut self) {
        self.inner.shutdown()
    }

    #[inline]
    fn poll(
        &mut self,
    ) -> Poll<
        Option<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>>,
        io::Error,
    > {
        Ok(self.inner.poll()?.map(|ev| {
            ev.map(|ev| match ev {
                ProtocolsHandlerEvent::Custom(ev) => ProtocolsHandlerEvent::Custom((self.map)(ev)),
                ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info } => {
                    ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info }
                }
            })
        }))
    }
}

/// Prototype for a `NodeHandlerWrapper`.
pub struct NodeHandlerWrapperBuilder<TProtoHandler>
where
    TProtoHandler: ProtocolsHandler,
{
    /// The underlying handler.
    handler: TProtoHandler,
    /// Timeout for incoming substreams negotiation.
    in_timeout: Duration,
    /// Timeout for outgoing substreams negotiation.
    out_timeout: Duration,
}

impl<TProtoHandler> NodeHandlerWrapperBuilder<TProtoHandler>
where
    TProtoHandler: ProtocolsHandler
{
    /// Sets the timeout to use when negotiating a protocol on an ingoing substream.
    #[inline]
    pub fn with_in_negotiation_timeout(mut self, timeout: Duration) -> Self {
        self.in_timeout = timeout;
        self
    }

    /// Sets the timeout to use when negotiating a protocol on an outgoing substream.
    #[inline]
    pub fn with_out_negotiation_timeout(mut self, timeout: Duration) -> Self {
        self.out_timeout = timeout;
        self
    }

    /// Builds the `NodeHandlerWrapper`.
    #[inline]
    pub fn build(self) -> NodeHandlerWrapper<TProtoHandler> {
        NodeHandlerWrapper {
            handler: self.handler,
            negotiating_in: Vec::new(),
            negotiating_out: Vec::new(),
            in_timeout: self.in_timeout,
            out_timeout: self.out_timeout,
            queued_dial_upgrades: Vec::new(),
            unique_dial_upgrade_id: 0,
        }
    }
}

/// Wraps around an implementation of `ProtocolsHandler`, and implements `NodeHandler`.
// TODO: add a caching system for protocols that are supported or not
pub struct NodeHandlerWrapper<TProtoHandler>
where
    TProtoHandler: ProtocolsHandler,
{
    /// The underlying handler.
    handler: TProtoHandler,
    /// Futures that upgrade incoming substreams.
    negotiating_in:
        Vec<Timeout<InboundUpgradeApply<TProtoHandler::Substream, TProtoHandler::InboundProtocol>>>,
    /// Futures that upgrade outgoing substreams. The first element of the tuple is the userdata
    /// to pass back once successfully opened.
    negotiating_out: Vec<(
        TProtoHandler::OutboundOpenInfo,
        Timeout<OutboundUpgradeApply<TProtoHandler::Substream, TProtoHandler::OutboundProtocol>>,
    )>,
    /// Timeout for incoming substreams negotiation.
    in_timeout: Duration,
    /// Timeout for outgoing substreams negotiation.
    out_timeout: Duration,
    /// For each outbound substream request, how to upgrade it. The first element of the tuple
    /// is the unique identifier (see `unique_dial_upgrade_id`).
    queued_dial_upgrades: Vec<(u64, TProtoHandler::OutboundProtocol)>,
    /// Unique identifier assigned to each queued dial upgrade.
    unique_dial_upgrade_id: u64,
}

impl<TProtoHandler> NodeHandler for NodeHandlerWrapper<TProtoHandler>
where
    TProtoHandler: ProtocolsHandler,
    <TProtoHandler::OutboundProtocol as OutboundUpgrade<<TProtoHandler as ProtocolsHandler>::Substream>>::Error: std::fmt::Debug
{
    type InEvent = TProtoHandler::InEvent;
    type OutEvent = TProtoHandler::OutEvent;
    type Substream = TProtoHandler::Substream;
    // The first element of the tuple is the unique upgrade identifier
    // (see `unique_dial_upgrade_id`).
    type OutboundOpenInfo = (u64, TProtoHandler::OutboundOpenInfo);

    fn inject_substream(
        &mut self,
        substream: Self::Substream,
        endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>,
    ) {
        match endpoint {
            NodeHandlerEndpoint::Listener => {
                let protocol = self.handler.listen_protocol();
                let upgrade = upgrade::apply_inbound(substream, protocol);
                let with_timeout = Timeout::new(upgrade, self.in_timeout);
                self.negotiating_in.push(with_timeout);
            }
            NodeHandlerEndpoint::Dialer((upgrade_id, user_data)) => {
                let pos = match self
                    .queued_dial_upgrades
                    .iter()
                    .position(|(id, _)| id == &upgrade_id)
                {
                    Some(p) => p,
                    None => {
                        debug_assert!(false, "Received an upgrade with an invalid upgrade ID");
                        return;
                    }
                };

                let (_, proto_upgrade) = self.queued_dial_upgrades.remove(pos);
                let upgrade = upgrade::apply_outbound(substream, proto_upgrade);
                let with_timeout = Timeout::new(upgrade, self.out_timeout);
                self.negotiating_out.push((user_data, with_timeout));
            }
        }
    }

    #[inline]
    fn inject_inbound_closed(&mut self) {
        self.handler.inject_inbound_closed();
    }

    fn inject_outbound_closed(&mut self, user_data: Self::OutboundOpenInfo) {
        let pos = match self
            .queued_dial_upgrades
            .iter()
            .position(|(id, _)| id == &user_data.0)
        {
            Some(p) => p,
            None => {
                debug_assert!(
                    false,
                    "Received an outbound closed error with an invalid upgrade ID"
                );
                return;
            }
        };

        self.queued_dial_upgrades.remove(pos);
        self.handler
            .inject_dial_upgrade_error(user_data.1, io::ErrorKind::ConnectionReset.into());
    }

    #[inline]
    fn inject_event(&mut self, event: Self::InEvent) {
        self.handler.inject_event(event);
    }

    #[inline]
    fn shutdown(&mut self) {
        self.handler.shutdown();
    }

    fn poll(&mut self) -> Poll<Option<NodeHandlerEvent<Self::OutboundOpenInfo, Self::OutEvent>>, io::Error> {
        // Continue negotiation of newly-opened substreams on the listening side.
        // We remove each element from `negotiating_in` one by one and add them back if not ready.
        for n in (0..self.negotiating_in.len()).rev() {
            let mut in_progress = self.negotiating_in.swap_remove(n);
            match in_progress.poll() {
                Ok(Async::Ready(upgrade)) =>
                    self.handler.inject_fully_negotiated_inbound(upgrade),
                Ok(Async::NotReady) => self.negotiating_in.push(in_progress),
                // TODO: return a diagnostic event?
                Err(_err) => {}
            }
        }

        // Continue negotiation of newly-opened substreams.
        // We remove each element from `negotiating_out` one by one and add them back if not ready.
        for n in (0..self.negotiating_out.len()).rev() {
            let (upgr_info, mut in_progress) = self.negotiating_out.swap_remove(n);
            match in_progress.poll() {
                Ok(Async::Ready(upgrade)) => {
                    self.handler.inject_fully_negotiated_outbound(upgrade, upgr_info);
                }
                Ok(Async::NotReady) => {
                    self.negotiating_out.push((upgr_info, in_progress));
                }
                Err(err) => {
                    let msg = format!("Error while upgrading: {:?}", err);
                    let err = io::Error::new(io::ErrorKind::Other, msg);
                    self.handler.inject_dial_upgrade_error(upgr_info, err);
                }
            }
        }

        // Poll the handler at the end so that we see the consequences of the method calls on
        // `self.handler`.
        match self.handler.poll()? {
            Async::Ready(Some(ProtocolsHandlerEvent::Custom(event))) => {
                return Ok(Async::Ready(Some(NodeHandlerEvent::Custom(event))));
            }
            Async::Ready(Some(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                upgrade,
                info,
            })) => {
                let id = self.unique_dial_upgrade_id;
                self.unique_dial_upgrade_id += 1;
                self.queued_dial_upgrades.push((id, upgrade));
                return Ok(Async::Ready(Some(
                    NodeHandlerEvent::OutboundSubstreamRequest((id, info)),
                )));
            }
            Async::Ready(None) => return Ok(Async::Ready(None)),
            Async::NotReady => (),
        };

        Ok(Async::NotReady)
    }
}

/// Implementation of `ProtocolsHandler` that combines two protocols into one.
#[derive(Debug, Clone)]
pub struct ProtocolsHandlerSelect<TProto1, TProto2> {
    proto1: TProto1,
    proto2: TProto2,
}

impl<TSubstream, TProto1, TProto2, TProto1Out, TProto2Out>
    ProtocolsHandler for ProtocolsHandlerSelect<TProto1, TProto2>
where
    TProto1: ProtocolsHandler<Substream = TSubstream>,
    TProto2: ProtocolsHandler<Substream = TSubstream>,
    TSubstream: AsyncRead + AsyncWrite,
    TProto1::InboundProtocol: InboundUpgrade<TSubstream, Output = TProto1Out>,
    TProto2::InboundProtocol: InboundUpgrade<TSubstream, Output = TProto2Out>,
    TProto1::OutboundProtocol: OutboundUpgrade<TSubstream, Output = TProto1Out>,
    TProto2::OutboundProtocol: OutboundUpgrade<TSubstream, Output = TProto2Out>
{
    type InEvent = EitherOutput<TProto1::InEvent, TProto2::InEvent>;
    type OutEvent = EitherOutput<TProto1::OutEvent, TProto2::OutEvent>;
    type Substream = TSubstream;
    type InboundProtocol = OrUpgrade<TProto1::InboundProtocol, TProto2::InboundProtocol>;
    type OutboundProtocol = EitherUpgrade<TProto1::OutboundProtocol, TProto2::OutboundProtocol>;
    type OutboundOpenInfo = EitherOutput<TProto1::OutboundOpenInfo, TProto2::OutboundOpenInfo>;

    #[inline]
    fn listen_protocol(&self) -> Self::InboundProtocol {
        let proto1 = self.proto1.listen_protocol();
        let proto2 = self.proto2.listen_protocol();
        proto1.or_inbound(proto2)
    }

    fn inject_fully_negotiated_outbound(&mut self, protocol: <Self::OutboundProtocol as OutboundUpgrade<TSubstream>>::Output, endpoint: Self::OutboundOpenInfo) {
        match (protocol, endpoint) {
            (EitherOutput::First(protocol), EitherOutput::First(info)) =>
                self.proto1.inject_fully_negotiated_outbound(protocol, info),
            (EitherOutput::Second(protocol), EitherOutput::Second(info)) =>
                self.proto2.inject_fully_negotiated_outbound(protocol, info),
            (EitherOutput::First(_), EitherOutput::Second(_)) =>
                panic!("wrong API usage: the protocol doesn't match the upgrade info"),
            (EitherOutput::Second(_), EitherOutput::First(_)) =>
                panic!("wrong API usage: the protocol doesn't match the upgrade info")
        }
    }

    fn inject_fully_negotiated_inbound(&mut self, protocol: <Self::InboundProtocol as InboundUpgrade<TSubstream>>::Output) {
        match protocol {
            EitherOutput::First(protocol) =>
                self.proto1.inject_fully_negotiated_inbound(protocol),
            EitherOutput::Second(protocol) =>
                self.proto2.inject_fully_negotiated_inbound(protocol)
        }
    }

    #[inline]
    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            EitherOutput::First(event) => self.proto1.inject_event(event),
            EitherOutput::Second(event) => self.proto2.inject_event(event),
        }
    }

    #[inline]
    fn inject_inbound_closed(&mut self) {
        self.proto1.inject_inbound_closed();
        self.proto2.inject_inbound_closed();
    }

    #[inline]
    fn inject_dial_upgrade_error(&mut self, info: Self::OutboundOpenInfo, error: io::Error) {
        match info {
            EitherOutput::First(info) => self.proto1.inject_dial_upgrade_error(info, error),
            EitherOutput::Second(info) => self.proto2.inject_dial_upgrade_error(info, error),
        }
    }

    #[inline]
    fn shutdown(&mut self) {
        self.proto1.shutdown();
        self.proto2.shutdown();
    }

    fn poll(&mut self) -> Poll<Option<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent>>, io::Error> {
        match self.proto1.poll()? {
            Async::Ready(Some(ProtocolsHandlerEvent::Custom(event))) => {
                return Ok(Async::Ready(Some(ProtocolsHandlerEvent::Custom(EitherOutput::First(event)))));
            },
            Async::Ready(Some(ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info})) => {
                return Ok(Async::Ready(Some(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    upgrade: EitherUpgrade::A(upgrade),
                    info: EitherOutput::First(info),
                })));
            },
            Async::Ready(None) => return Ok(Async::Ready(None)),
            Async::NotReady => ()
        };

        match self.proto2.poll()? {
            Async::Ready(Some(ProtocolsHandlerEvent::Custom(event))) => {
                return Ok(Async::Ready(Some(ProtocolsHandlerEvent::Custom(EitherOutput::Second(event)))));
            },
            Async::Ready(Some(ProtocolsHandlerEvent::OutboundSubstreamRequest { upgrade, info })) => {
                return Ok(Async::Ready(Some(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    upgrade: EitherUpgrade::B(upgrade),
                    info: EitherOutput::Second(info),
                })));
            },
            Async::Ready(None) => return Ok(Async::Ready(None)),
            Async::NotReady => ()
        };

        Ok(Async::NotReady)
    }
}
