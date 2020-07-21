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

use crate::{Multiaddr, PeerId};
use std::task::{Context, Poll};
use super::{Connected, SubstreamEndpoint};

/// The interface of a connection handler.
///
/// Each handler is responsible for a single connection.
pub trait ConnectionHandler {
    /// The inbound type of events used to notify the handler through the `Network`.
    ///
    /// See also [`EstablishedConnection::notify_handler`](super::EstablishedConnection::notify_handler)
    /// and [`ConnectionHandler::inject_event`].
    type InEvent;
    /// The outbound type of events that the handler emits to the `Network`
    /// through [`ConnectionHandler::poll`].
    ///
    /// See also [`NetworkEvent::ConnectionEvent`](crate::network::NetworkEvent::ConnectionEvent).
    type OutEvent;
    /// The type of errors that the handler can produce when polled by the `Network`.
    type Error;
    /// The type of the substream containing the data.
    type Substream;
    /// Information about a substream. Can be sent to the handler through a `SubstreamEndpoint`,
    /// and will be passed back in `inject_substream` or `inject_outbound_closed`.
    type OutboundOpenInfo;

    /// Sends a new substream to the handler.
    ///
    /// The handler is responsible for upgrading the substream to whatever protocol it wants.
    ///
    /// # Panic
    ///
    /// Implementations are allowed to panic in the case of dialing if the `user_data` in
    /// `endpoint` doesn't correspond to what was returned earlier when polling, or is used
    /// multiple times.
    fn inject_substream(&mut self, substream: Self::Substream, endpoint: SubstreamEndpoint<Self::OutboundOpenInfo>);

    /// Notifies the handler of an event.
    fn inject_event(&mut self, event: Self::InEvent);

    /// Notifies the handler of a change in the address of the remote.
    fn inject_address_change(&mut self, new_address: &Multiaddr);

    /// Polls the handler for events.
    ///
    /// Returning an error will close the connection to the remote.
    fn poll(&mut self, cx: &mut Context<'_>)
        -> Poll<Result<ConnectionHandlerEvent<Self::OutboundOpenInfo, Self::OutEvent>, Self::Error>>;

    /// Polls the handler to make progress towards closing the connection.
    ///
    /// When a connection is actively closed, the handler can perform
    /// a graceful shutdown of the connection by draining the I/O
    /// activity, e.g. allowing in-flight requests to complete without
    /// accepting new ones, possibly signaling the remote that it
    /// should direct further requests elsewhere.
    ///
    /// The handler can also use the opportunity to flush any buffers
    /// or clean up any other (asynchronous) resources before the
    /// connection is ultimately dropped and closed on the transport
    /// layer.
    ///
    /// While closing, new inbound substreams are rejected and the
    /// handler is unable to request new outbound substreams as
    /// per the return type of `poll_close`.
    ///
    /// The handler signals its readiness for the connection
    /// to be closed by returning `Ready(Ok(None))`, which is the
    /// default implementation. Hence, by default, connection
    /// shutdown is not delayed and may result in ungraceful
    /// interruption of ongoing I/O.
    ///
    /// > **Note**: Once `poll_close()` is invoked, the handler is no
    /// > longer `poll()`ed.
    fn poll_close(&mut self, _: &mut Context)
        -> Poll<Result<Option<Self::OutEvent>, Self::Error>>
    {
        Poll::Ready(Ok(None))
    }
}

/// Prototype for a `ConnectionHandler`.
pub trait IntoConnectionHandler<TConnInfo = PeerId> {
    /// The node handler.
    type Handler: ConnectionHandler;

    /// Builds the node handler.
    ///
    /// The implementation is given a `Connected` value that holds information about
    /// the newly established connection for which a handler should be created.
    fn into_handler(self, connected: &Connected<TConnInfo>) -> Self::Handler;
}

impl<T, TConnInfo> IntoConnectionHandler<TConnInfo> for T
where
    T: ConnectionHandler
{
    type Handler = Self;

    fn into_handler(self, _: &Connected<TConnInfo>) -> Self {
        self
    }
}

/// Event produced by a handler.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ConnectionHandlerEvent<TOutboundOpenInfo, TCustom> {
    /// Require a new outbound substream to be opened with the remote.
    OutboundSubstreamRequest(TOutboundOpenInfo),

    /// Other event.
    Custom(TCustom),

    /// Initiate connection shutdown.
    Close,
}

/// Event produced by a handler.
impl<TOutboundOpenInfo, TCustom> ConnectionHandlerEvent<TOutboundOpenInfo, TCustom> {
    /// If this is `OutboundSubstreamRequest`, maps the content to something else.
    pub fn map_outbound_open_info<F, I>(self, map: F) -> ConnectionHandlerEvent<I, TCustom>
    where F: FnOnce(TOutboundOpenInfo) -> I
    {
        match self {
            ConnectionHandlerEvent::OutboundSubstreamRequest(val) => {
                ConnectionHandlerEvent::OutboundSubstreamRequest(map(val))
            },
            ConnectionHandlerEvent::Custom(val) => ConnectionHandlerEvent::Custom(val),
            ConnectionHandlerEvent::Close => ConnectionHandlerEvent::Close,
        }
    }

    /// If this is `Custom`, maps the content to something else.
    pub fn map_custom<F, I>(self, map: F) -> ConnectionHandlerEvent<TOutboundOpenInfo, I>
    where F: FnOnce(TCustom) -> I
    {
        match self {
            ConnectionHandlerEvent::OutboundSubstreamRequest(val) => {
                ConnectionHandlerEvent::OutboundSubstreamRequest(val)
            },
            ConnectionHandlerEvent::Custom(val) => ConnectionHandlerEvent::Custom(map(val)),
            ConnectionHandlerEvent::Close => ConnectionHandlerEvent::Close,
        }
    }
}

