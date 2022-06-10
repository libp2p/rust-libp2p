// Copyright 2020 Parity Technologies (UK) Ltd.
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

mod error;
mod handler_wrapper;
mod listeners;
mod substream;

pub(crate) mod pool;

pub use error::{
    ConnectionError, PendingConnectionError, PendingInboundConnectionError,
    PendingOutboundConnectionError,
};
pub use listeners::{ListenersEvent, ListenersStream};
pub use pool::{ConnectionCounters, ConnectionLimits};
pub use pool::{EstablishedConnection, PendingConnection};
pub use substream::{Close, Substream, SubstreamEndpoint};

use crate::handler::ConnectionHandler;
use handler_wrapper::HandlerWrapper;
use libp2p_core::connection::ConnectedPoint;
use libp2p_core::multiaddr::Multiaddr;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_core::upgrade;
use libp2p_core::PeerId;
use std::{error::Error, fmt, pin::Pin, task::Context, task::Poll};
use substream::{Muxing, SubstreamEvent};

/// Information about a successfully established connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connected {
    /// The connected endpoint, including network address information.
    pub endpoint: ConnectedPoint,
    /// Information obtained from the transport.
    pub peer_id: PeerId,
}

/// Event generated by a [`Connection`].
#[derive(Debug, Clone)]
pub enum Event<T> {
    /// Event generated by the [`ConnectionHandler`].
    Handler(T),
    /// Address of the remote has changed.
    AddressChange(Multiaddr),
}

/// A multiplexed connection to a peer with an associated [`ConnectionHandler`].
pub struct Connection<THandler>
where
    THandler: ConnectionHandler,
{
    /// Node that handles the muxing.
    muxing: substream::Muxing<StreamMuxerBox, handler_wrapper::OutboundOpenInfo<THandler>>,
    /// Handler that processes substreams.
    handler: HandlerWrapper<THandler>,
}

impl<THandler> fmt::Debug for Connection<THandler>
where
    THandler: ConnectionHandler + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("muxing", &self.muxing)
            .field("handler", &self.handler)
            .finish()
    }
}

impl<THandler> Unpin for Connection<THandler> where THandler: ConnectionHandler {}

impl<THandler> Connection<THandler>
where
    THandler: ConnectionHandler,
{
    /// Builds a new `Connection` from the given substream multiplexer
    /// and connection handler.
    pub fn new(
        muxer: StreamMuxerBox,
        handler: THandler,
        substream_upgrade_protocol_override: Option<upgrade::Version>,
        max_negotiating_inbound_streams: usize,
    ) -> Self {
        let wrapped_handler = HandlerWrapper::new(
            handler,
            substream_upgrade_protocol_override,
            max_negotiating_inbound_streams,
        );
        Connection {
            muxing: Muxing::new(muxer),
            handler: wrapped_handler,
        }
    }

    /// Notifies the connection handler of an event.
    pub fn inject_event(&mut self, event: THandler::InEvent) {
        self.handler.inject_event(event);
    }

    /// Begins an orderly shutdown of the connection, returning the connection
    /// handler and a `Future` that resolves when connection shutdown is complete.
    pub fn close(self) -> (THandler, Close<StreamMuxerBox>) {
        (
            self.handler.into_connection_handler(),
            self.muxing.close().0,
        )
    }

    /// Polls the handler and the substream, forwarding events from the former to the latter and
    /// vice versa.
    pub fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Event<THandler::OutEvent>, ConnectionError<THandler::Error>>> {
        loop {
            // Poll the handler for new events.
            match self.handler.poll(cx) {
                Poll::Pending => {}
                Poll::Ready(Ok(handler_wrapper::Event::OutboundSubstreamRequest(user_data))) => {
                    self.muxing.open_substream(user_data);
                    continue;
                }
                Poll::Ready(Ok(handler_wrapper::Event::Custom(event))) => {
                    return Poll::Ready(Ok(Event::Handler(event)));
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err.into())),
            }

            // Perform I/O on the connection through the muxer, informing the handler
            // of new substreams.
            match self.muxing.poll(cx) {
                Poll::Pending => {}
                Poll::Ready(Ok(SubstreamEvent::InboundSubstream { substream })) => {
                    self.handler
                        .inject_substream(substream, SubstreamEndpoint::Listener);
                    continue;
                }
                Poll::Ready(Ok(SubstreamEvent::OutboundSubstream {
                    user_data,
                    substream,
                })) => {
                    let endpoint = SubstreamEndpoint::Dialer(user_data);
                    self.handler.inject_substream(substream, endpoint);
                    continue;
                }
                Poll::Ready(Ok(SubstreamEvent::AddressChange(address))) => {
                    self.handler.inject_address_change(&address);
                    return Poll::Ready(Ok(Event::AddressChange(address)));
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(ConnectionError::IO(err))),
            }

            return Poll::Pending;
        }
    }
}

/// Borrowed information about an incoming connection currently being negotiated.
#[derive(Debug, Copy, Clone)]
pub struct IncomingInfo<'a> {
    /// Local connection address.
    pub local_addr: &'a Multiaddr,
    /// Address used to send back data to the remote.
    pub send_back_addr: &'a Multiaddr,
}

impl<'a> IncomingInfo<'a> {
    /// Builds the [`ConnectedPoint`] corresponding to the incoming connection.
    pub fn create_connected_point(&self) -> ConnectedPoint {
        ConnectedPoint::Listener {
            local_addr: self.local_addr.clone(),
            send_back_addr: self.send_back_addr.clone(),
        }
    }
}

/// Information about a connection limit.
#[derive(Debug, Clone)]
pub struct ConnectionLimit {
    /// The maximum number of connections.
    pub limit: u32,
    /// The current number of connections.
    pub current: u32,
}

impl fmt::Display for ConnectionLimit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.current, self.limit)
    }
}

/// A `ConnectionLimit` can represent an error if it has been exceeded.
impl Error for ConnectionLimit {}
