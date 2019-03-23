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

use crate::muxing::StreamMuxer;
use crate::nodes::node::{NodeEvent, NodeStream, Substream, Close};
use futures::prelude::*;
use std::{error, fmt, io};

mod tests;

/// Handler for the substreams of a node.
// TODO: right now it is possible for a node handler to be built, then shut down right after if we
//       realize we dialed the wrong peer for example; this could be surprising and should either
//       be documented or changed (favouring the "documented" right now)
pub trait NodeHandler {
    /// Custom event that can be received from the outside.
    type InEvent;
    /// Custom event that can be produced by the handler and that will be returned by the swarm.
    type OutEvent;
    /// Error that can happen during the processing of the node.
    type Error;
    /// The type of the substream containing the data.
    type Substream;
    /// Information about a substream. Can be sent to the handler through a `NodeHandlerEndpoint`,
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
    fn inject_substream(&mut self, substream: Self::Substream, endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>);

    /// Injects an event coming from the outside into the handler.
    fn inject_event(&mut self, event: Self::InEvent);

    /// Should behave like `Stream::poll()`.
    ///
    /// Returning an error will close the connection to the remote.
    fn poll(&mut self) -> Poll<NodeHandlerEvent<Self::OutboundOpenInfo, Self::OutEvent>, Self::Error>;
}

/// Endpoint for a received substream.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NodeHandlerEndpoint<TOutboundOpenInfo> {
    Dialer(TOutboundOpenInfo),
    Listener,
}

impl<TOutboundOpenInfo> NodeHandlerEndpoint<TOutboundOpenInfo> {
    /// Returns true for `Dialer`.
    #[inline]
    pub fn is_dialer(&self) -> bool {
        match self {
            NodeHandlerEndpoint::Dialer(_) => true,
            NodeHandlerEndpoint::Listener => false,
        }
    }

    /// Returns true for `Listener`.
    #[inline]
    pub fn is_listener(&self) -> bool {
        match self {
            NodeHandlerEndpoint::Dialer(_) => false,
            NodeHandlerEndpoint::Listener => true,
        }
    }
}

/// Event produced by a handler.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NodeHandlerEvent<TOutboundOpenInfo, TCustom> {
    /// Require a new outbound substream to be opened with the remote.
    OutboundSubstreamRequest(TOutboundOpenInfo),

    /// Other event.
    Custom(TCustom),
}

/// Event produced by a handler.
impl<TOutboundOpenInfo, TCustom> NodeHandlerEvent<TOutboundOpenInfo, TCustom> {
    /// If this is `OutboundSubstreamRequest`, maps the content to something else.
    #[inline]
    pub fn map_outbound_open_info<F, I>(self, map: F) -> NodeHandlerEvent<I, TCustom>
    where F: FnOnce(TOutboundOpenInfo) -> I
    {
        match self {
            NodeHandlerEvent::OutboundSubstreamRequest(val) => {
                NodeHandlerEvent::OutboundSubstreamRequest(map(val))
            },
            NodeHandlerEvent::Custom(val) => NodeHandlerEvent::Custom(val),
        }
    }

    /// If this is `Custom`, maps the content to something else.
    #[inline]
    pub fn map_custom<F, I>(self, map: F) -> NodeHandlerEvent<TOutboundOpenInfo, I>
    where F: FnOnce(TCustom) -> I
    {
        match self {
            NodeHandlerEvent::OutboundSubstreamRequest(val) => {
                NodeHandlerEvent::OutboundSubstreamRequest(val)
            },
            NodeHandlerEvent::Custom(val) => NodeHandlerEvent::Custom(map(val)),
        }
    }
}

/// A node combined with an implementation of `NodeHandler`.
pub struct HandledNode<TMuxer, THandler>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream = Substream<TMuxer>>,
{
    /// Node that handles the muxing.
    node: NodeStream<TMuxer, THandler::OutboundOpenInfo>,
    /// Handler that processes substreams.
    handler: THandler,
}

impl<TMuxer, THandler> fmt::Debug for HandledNode<TMuxer, THandler>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream = Substream<TMuxer>> + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HandledNode")
            .field("node", &self.node)
            .field("handler", &self.handler)
            .finish()
    }
}

impl<TMuxer, THandler> HandledNode<TMuxer, THandler>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream = Substream<TMuxer>>,
{
    /// Builds a new `HandledNode`.
    #[inline]
    pub fn new(muxer: TMuxer, handler: THandler) -> Self {
        HandledNode {
            node: NodeStream::new(muxer),
            handler,
        }
    }

    /// Returns a reference to the `NodeHandler`
    pub fn handler(&self) -> &THandler {
        &self.handler
    }

    /// Returns a mutable reference to the `NodeHandler`
    pub fn handler_mut(&mut self) -> &mut THandler {
        &mut self.handler
    }

    /// Injects an event to the handler. Has no effect if the handler is closing.
    #[inline]
    pub fn inject_event(&mut self, event: THandler::InEvent) {
        self.handler.inject_event(event);
    }

    /// Returns `true` if the remote has shown any sign of activity after the muxer has been open.
    ///
    /// See `StreamMuxer::is_remote_acknowledged`.
    pub fn is_remote_acknowledged(&self) -> bool {
        self.node.is_remote_acknowledged()
    }

    /// Indicates to the handled node that it should shut down. After calling this method, the
    /// `Stream` will end in the not-so-distant future.
    pub fn close(self) -> Close<TMuxer> {
        self.node.close().0
    }

    /// API similar to `Future::poll` that polls the node for events.
    pub fn poll(&mut self) -> Poll<THandler::OutEvent, HandledNodeError<THandler::Error>> {
        loop {
            let mut node_not_ready = false;

            match self.node.poll().map_err(HandledNodeError::Node)? {
                Async::NotReady => node_not_ready = true,
                Async::Ready(NodeEvent::InboundSubstream { substream }) => {
                    self.handler.inject_substream(substream, NodeHandlerEndpoint::Listener)
                }
                Async::Ready(NodeEvent::OutboundSubstream { user_data, substream }) => {
                    let endpoint = NodeHandlerEndpoint::Dialer(user_data);
                    self.handler.inject_substream(substream, endpoint)
                }
            }

            match self.handler.poll().map_err(HandledNodeError::Handler)? {
                Async::NotReady => {
                    if node_not_ready {
                        break
                    }
                }
                Async::Ready(NodeHandlerEvent::OutboundSubstreamRequest(user_data)) => {
                    self.node.open_substream(user_data);
                }
                Async::Ready(NodeHandlerEvent::Custom(event)) => {
                    return Ok(Async::Ready(event));
                }
            }
        }

        Ok(Async::NotReady)
    }
}

/// Error that can happen when polling a `HandledNode`.
#[derive(Debug)]
pub enum HandledNodeError<THandlerErr> {
    /// An error happend in the stream muxer.
    // TODO: eventually this should also be a custom error
    Node(io::Error),
    /// An error happened in the handler of the connection to the node.
    Handler(THandlerErr),
}

impl<THandlerErr> fmt::Display for HandledNodeError<THandlerErr>
where THandlerErr: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandledNodeError::Node(err) => write!(f, "{}", err),
            HandledNodeError::Handler(err) => write!(f, "{}", err),
        }
    }
}

impl<THandlerErr> error::Error for HandledNodeError<THandlerErr>
where THandlerErr: error::Error + 'static
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            HandledNodeError::Node(err) => Some(err),
            HandledNodeError::Handler(err) => Some(err),
        }
    }
}
