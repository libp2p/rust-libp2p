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

use muxing::StreamMuxer;
use nodes::node::{NodeEvent, NodeStream, Substream};
use futures::prelude::*;
use std::io::Error as IoError;
use Multiaddr;

/// Handler for the substreams of a node.
///
/// > Note: When implementing the various methods, don't forget that you have to register the
/// > task that was the latest to poll and notify it.
// TODO: right now it is possible for a node handler to be built, then shut down right after if we
//       realize we dialed the wrong peer for example ; this could be surprising and should either
//       be documented or changed (favouring the "documented" right now)
pub trait NodeHandler<TSubstream> {
    /// Custom event that can be received from the outside.
    type InEvent;
    /// Custom event that can be produced by the handler and that will be returned by the swarm.
    type OutEvent;
    /// Information about a substream. Can be sent to the handler through a `NodeHandlerEndpoint`,
    /// and will be passed back in `inject_substream` or `inject_outbound_closed`.
    type OutboundOpenInfo;

    /// Sends a new substream to the handler.
    ///
    /// The handler is responsible for upgrading the substream to whatever protocol it wants.
    fn inject_substream(&mut self, substream: TSubstream, endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>);

    /// Indicates the handler that the inbound part of the muxer has been closed, and that
    /// therefore no more inbound substream will be produced.
    fn inject_inbound_closed(&mut self);

    /// Indicates the handler that an outbound substream failed to open because the outbound
    /// part of the muxer has been closed.
    fn inject_outbound_closed(&mut self, user_data: Self::OutboundOpenInfo);

    /// Indicates the handler that the multiaddr future has resolved.
    fn inject_multiaddr(&mut self, multiaddr: Result<Multiaddr, IoError>);

    /// Injects an event coming from the outside in the handler.
    fn inject_event(&mut self, event: Self::InEvent);

    /// Indicates the node that it should shut down. After that, it is expected that `poll()`
    /// returns `Ready(None)` as soon as possible.
    ///
    /// This method allows an implementation to perform a graceful shutdown of the substreams, and
    /// send back various events.
    fn shutdown(&mut self);

    /// Should behave like `Stream::poll()`. Should close if no more event can be produced and the
    /// node should be closed.
    fn poll(&mut self) -> Poll<Option<NodeHandlerEvent<Self::OutboundOpenInfo, Self::OutEvent>>, IoError>;
}

/// Endpoint for a received substream.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NodeHandlerEndpoint<TOutboundOpenInfo> {
    Dialer(TOutboundOpenInfo),
    Listener,
}

/// Event produces by a handler.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum NodeHandlerEvent<TOutboundOpenInfo, TCustom> {
    /// Require a new outbound substream to be opened with the remote.
    OutboundSubstreamRequest(TOutboundOpenInfo),

    /// Other event.
    Custom(TCustom),
}

/// Event produces by a handler.
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
// TODO: impl Debug
pub struct HandledNode<TMuxer, TAddrFut, THandler>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream<TMuxer>>,
{
    /// Node that handles the muxing. Can be `None` if the handled node is shutting down.
    node: Option<NodeStream<TMuxer, TAddrFut, THandler::OutboundOpenInfo>>,
    /// Handler that processes substreams.
    handler: THandler,
}

impl<TMuxer, TAddrFut, THandler> HandledNode<TMuxer, TAddrFut, THandler>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream<TMuxer>>,
    TAddrFut: Future<Item = Multiaddr, Error = IoError>,
{
    /// Builds a new `HandledNode`.
    #[inline]
    pub fn new(muxer: TMuxer, multiaddr_future: TAddrFut, handler: THandler) -> Self {
        HandledNode {
            node: Some(NodeStream::new(muxer, multiaddr_future)),
            handler,
        }
    }

    /// Injects an event to the handler.
    #[inline]
    pub fn inject_event(&mut self, event: THandler::InEvent) {
        self.handler.inject_event(event);
    }

    /// Returns true if the inbound channel of the muxer is closed.
    ///
    /// If `true` is returned, then no more inbound substream will be received.
    #[inline]
    pub fn is_inbound_closed(&self) -> bool {
        self.node.as_ref().map(|n| n.is_inbound_closed()).unwrap_or(true)
    }

    /// Returns true if the outbound channel of the muxer is closed.
    ///
    /// If `true` is returned, then no more outbound substream will be opened.
    #[inline]
    pub fn is_outbound_closed(&self) -> bool {
        self.node.as_ref().map(|n| n.is_outbound_closed()).unwrap_or(true)
    }

    /// Returns true if the handled node is in the process of shutting down.
    #[inline]
    pub fn is_shutting_down(&self) -> bool {
        self.node.is_none()
    }

    /// Indicates the handled node that it should shut down. After calling this method, the
    /// `Stream` will end in the not-so-distant future.
    ///
    /// After this method returns, `is_shutting_down()` should return true.
    pub fn shutdown(&mut self) {
        if let Some(node) = self.node.take() {
            for user_data in node.close() {
                self.handler.inject_outbound_closed(user_data);
            }
        }

        self.handler.shutdown();
    }
}

impl<TMuxer, TAddrFut, THandler> Stream for HandledNode<TMuxer, TAddrFut, THandler>
where
    TMuxer: StreamMuxer,
    THandler: NodeHandler<Substream<TMuxer>>,
    TAddrFut: Future<Item = Multiaddr, Error = IoError>,
{
    type Item = THandler::OutEvent;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let mut node_not_ready = false;

            match self.node.as_mut().map(|n| n.poll()) {
                Some(Ok(Async::NotReady)) | None => {},
                Some(Ok(Async::Ready(Some(NodeEvent::InboundSubstream { substream })))) => {
                    self.handler.inject_substream(substream, NodeHandlerEndpoint::Listener);
                },
                Some(Ok(Async::Ready(Some(NodeEvent::OutboundSubstream { user_data, substream })))) => {
                    let endpoint = NodeHandlerEndpoint::Dialer(user_data);
                    self.handler.inject_substream(substream, endpoint);
                },
                Some(Ok(Async::Ready(None))) => {
                    node_not_ready = true;
                    self.node = None;
                    self.handler.shutdown();
                },
                Some(Ok(Async::Ready(Some(NodeEvent::Multiaddr(result))))) => {
                    self.handler.inject_multiaddr(result);
                },
                Some(Ok(Async::Ready(Some(NodeEvent::OutboundClosed { user_data })))) => {
                    self.handler.inject_outbound_closed(user_data);
                },
                Some(Ok(Async::Ready(Some(NodeEvent::InboundClosed)))) => {
                    self.handler.inject_inbound_closed();
                },
                Some(Err(err)) => {
                    self.node = None;
                    return Err(err);
                },
            }

            match self.handler.poll() {
                Ok(Async::NotReady) => {
                    if node_not_ready {
                        break;
                    }
                },
                Ok(Async::Ready(Some(NodeHandlerEvent::OutboundSubstreamRequest(user_data)))) => {
                    if let Some(node) = self.node.as_mut() {
                        match node.open_substream(user_data) {
                            Ok(()) => (),
                            Err(user_data) => self.handler.inject_outbound_closed(user_data),
                        }
                    } else {
                        self.handler.inject_outbound_closed(user_data);
                    }
                },
                Ok(Async::Ready(Some(NodeHandlerEvent::Custom(event)))) => {
                    return Ok(Async::Ready(Some(event)));
                },
                Ok(Async::Ready(None)) => {
                    return Ok(Async::Ready(None));
                },
                Err(err) => {
                    return Err(err);
                },
            }
        }

        Ok(Async::NotReady)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future;
    use muxing::StreamMuxer;
    use tokio::runtime::current_thread;

    // TODO: move somewhere? this could be useful as a dummy
    struct InstaCloseMuxer;
    impl StreamMuxer for InstaCloseMuxer {
        type Substream = ();
        type OutboundSubstream = ();
        fn poll_inbound(&self) -> Poll<Option<Self::Substream>, IoError> { Ok(Async::Ready(None)) }
        fn open_outbound(&self) -> Self::OutboundSubstream { () }
        fn poll_outbound(&self, _: &mut Self::OutboundSubstream) -> Poll<Option<Self::Substream>, IoError> { Ok(Async::Ready(None)) }
        fn destroy_outbound(&self, _: Self::OutboundSubstream) {}
        fn read_substream(&self, _: &mut Self::Substream, _: &mut [u8]) -> Result<usize, IoError> { panic!() }
        fn write_substream(&self, _: &mut Self::Substream, _: &[u8]) -> Result<usize, IoError> { panic!() }
        fn flush_substream(&self, _: &mut Self::Substream) -> Result<(), IoError> { panic!() }
        fn shutdown_substream(&self, _: &mut Self::Substream) -> Poll<(), IoError> { panic!() }
        fn destroy_substream(&self, _: Self::Substream) { panic!() }
        fn close_inbound(&self) {}
        fn close_outbound(&self) {}
    }

    #[test]
    fn proper_shutdown() {
        // Test that `shutdown()` is properly called on the handler once a node stops.
        struct Handler {
            did_substream_attempt: bool,
            inbound_closed: bool,
            substream_attempt_cancelled: bool,
            shutdown_called: bool,
        };
        impl<T> NodeHandler<T> for Handler {
            type InEvent = ();
            type OutEvent = ();
            type OutboundOpenInfo = ();
            fn inject_substream(&mut self, _: T, _: NodeHandlerEndpoint<()>) { panic!() }
            fn inject_inbound_closed(&mut self) {
                assert!(!self.inbound_closed);
                self.inbound_closed = true;
            }
            fn inject_outbound_closed(&mut self, _: ()) {
                assert!(!self.substream_attempt_cancelled);
                self.substream_attempt_cancelled = true;
            }
            fn inject_multiaddr(&mut self, _: Result<Multiaddr, IoError>) {}
            fn inject_event(&mut self, _: Self::InEvent) { panic!() }
            fn shutdown(&mut self) {
                assert!(self.inbound_closed);
                assert!(self.substream_attempt_cancelled);
                self.shutdown_called = true;
            }
            fn poll(&mut self) -> Poll<Option<NodeHandlerEvent<(), ()>>, IoError> {
                if self.shutdown_called {
                    Ok(Async::Ready(None))
                } else if !self.did_substream_attempt {
                    self.did_substream_attempt = true;
                    Ok(Async::Ready(Some(NodeHandlerEvent::OutboundSubstreamRequest(()))))
                } else {
                    Ok(Async::NotReady)
                }
            }
        }
        impl Drop for Handler {
            fn drop(&mut self) {
                assert!(self.shutdown_called);
            }
        }

        let handled = HandledNode::new(InstaCloseMuxer, future::empty(), Handler {
            did_substream_attempt: false,
            inbound_closed: false,
            substream_attempt_cancelled: false,
            shutdown_called: false,
        });

        current_thread::Runtime::new().unwrap().block_on(handled.for_each(|_| Ok(()))).unwrap();
    }
}
