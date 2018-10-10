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

    /// Indicates to the handler that the inbound part of the muxer has been closed, and that
    /// therefore no more inbound substream will be produced.
    fn inject_inbound_closed(&mut self);

    /// Indicates to the handler that an outbound substream failed to open because the outbound
    /// part of the muxer has been closed.
    fn inject_outbound_closed(&mut self, user_data: Self::OutboundOpenInfo);

    /// Indicates to the handler that the multiaddr future has resolved.
    fn inject_multiaddr(&mut self, multiaddr: Result<Multiaddr, IoError>);

    /// Injects an event coming from the outside into the handler.
    fn inject_event(&mut self, event: Self::InEvent);

    /// Indicates to the node that it should shut down. After that, it is expected that `poll()`
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

    /// Indicates to the handled node that it should shut down. After calling this method, the
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
			println!("[HandledNode, poll] polling nodestream");
            match self.node.as_mut().map(|n| n.poll()) {
                Some(Ok(Async::NotReady)) | None => {},
                Some(Ok(Async::Ready(Some(NodeEvent::InboundSubstream { substream })))) => {
					println!("[HandledNode, poll] node yielded InboundSusbstream");

                    self.handler.inject_substream(substream, NodeHandlerEndpoint::Listener);
                },
                Some(Ok(Async::Ready(Some(NodeEvent::OutboundSubstream { user_data, substream })))) => {
					println!("[HandledNode, poll] node yielded OutboundSubstream");

                    let endpoint = NodeHandlerEndpoint::Dialer(user_data);
                    self.handler.inject_substream(substream, endpoint);
                },
                Some(Ok(Async::Ready(None))) => {
					println!("[HandledNode, poll] node yielded Ready(None)");

                    node_not_ready = true;
                    self.node = None;
                    self.handler.shutdown();
                },
                Some(Ok(Async::Ready(Some(NodeEvent::Multiaddr(result))))) => {
					println!("[HandledNode, poll] node yielded MultiAddr");
                    self.handler.inject_multiaddr(result);
                },
                Some(Ok(Async::Ready(Some(NodeEvent::OutboundClosed { user_data })))) => {
					println!("[HandledNode, poll] node yielded OutboundClosed");
                    self.handler.inject_outbound_closed(user_data);
                },
                Some(Ok(Async::Ready(Some(NodeEvent::InboundClosed)))) => {
					println!("[HandledNode, poll] node yielded InboundClosed");

                    self.handler.inject_inbound_closed();
                },
                Some(Err(err)) => {
                    self.node = None;
                    return Err(err);
                },
            }

			println!("[HandledNode, poll] polling handler");
            match self.handler.poll() {
                Ok(Async::NotReady) => {
                    if node_not_ready {
                        break;
                    }
                },
                Ok(Async::Ready(Some(NodeHandlerEvent::OutboundSubstreamRequest(user_data)))) => {
					println!("[HandledNode, poll] handler yielded an OutboundSubstreamRequest");
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
    use tokio::runtime::current_thread;
    use tests::dummy_muxer::{DummyMuxer, DummyConnectionState};
	use std::io;

    #[derive(Debug, PartialEq, Copy, Clone)]
	// TODO figure out if this is needed and in any case rename it to something sensible
    enum Event {
        Custom,
        Accept
    }
	#[derive(Debug, PartialEq, Clone, Copy)]
	enum HandlerState {
		NotReady,
		Ready(Option<NodeHandlerEvent<(), Event>>),
		Err,
	}
    #[derive(Debug, PartialEq, Clone)]
    struct Handler {
        events: Vec<Event>,
		state: Option<HandlerState>,
    }

	impl Handler {
		fn set_state(&mut self, state: HandlerState) {
			println!("[Handler, set_state] changing state from {:?} => {:?}", self.state, state);
			self.state = Some(state);
		}
	}

    impl Default for Handler {
        fn default() -> Self {
            Handler {
                events: Vec::new(),
				state: None,
            }
        }
    }

    impl<TAddrFut> HandledNode<DummyMuxer, TAddrFut, Handler>
    where
        TAddrFut: Future<Item = Multiaddr, Error = IoError>,
    {
        fn events(&self) -> Vec<Event> {
            self.handler.events.clone()
        }
    }

    impl<T> NodeHandler<T> for Handler {
        type InEvent = Event;
        type OutEvent = Event;
        type OutboundOpenInfo = ();
        fn inject_substream(&mut self, _: T, _: NodeHandlerEndpoint<()>) { panic!() }
        fn inject_inbound_closed(&mut self) { }
        fn inject_outbound_closed(&mut self, _: ()) { }
        fn inject_multiaddr(&mut self, _: Result<Multiaddr, IoError>) {}
        fn inject_event(&mut self, inevent: Self::InEvent) {
            self.events.push(inevent) // TODO: not sure I need this anymore
         }
        fn shutdown(&mut self) {}
        fn poll(&mut self) -> Poll<Option<NodeHandlerEvent<(), Event>>, IoError> {
			match self.state {
				Some(state) => match state {
					HandlerState::NotReady => Ok(Async::NotReady),
					HandlerState::Ready(None) => Ok(Async::Ready(None)),
					HandlerState::Ready(Some(event)) => Ok(Async::Ready(Some(event))),
					HandlerState::Err => {Err(io::Error::new(io::ErrorKind::Other, "oh noes"))},
				},
				None => Ok(Async::NotReady)
			}
			// TODO: refactor this stuff to use `with_handler_state` properly
            // if let Some(custom_e) = self.events.pop() {
            //     return Ok(Async::Ready(Some(NodeHandlerEvent::Custom(custom_e))));
            // }
        }
    }

    struct TestBuilder {
        addr: Multiaddr,
        muxer: DummyMuxer,
        handler: Handler,
    }

    impl TestBuilder {
        fn new() -> Self {
            TestBuilder {
                addr: "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr"),
                muxer: DummyMuxer::new(),
                handler: Handler::default(),
            }
        }

        fn with_muxer_inbound_state(&mut self, state: DummyConnectionState) -> &mut Self {
            self.muxer.set_inbound_connection_state(state);
            self
        }

        fn with_muxer_outbound_state(&mut self, state: DummyConnectionState) -> &mut Self {
            self.muxer.set_outbound_connection_state(state);
            self
        }

		fn with_handler_state(&mut self, state: HandlerState) -> &mut Self {
			self.handler.set_state(state);
			self
		}

        fn handled_node(&mut self)
        -> HandledNode<DummyMuxer, impl Future<Item = Multiaddr, Error = IoError>, Handler> {
            HandledNode::new(self.muxer.clone(), future::ok(self.addr.clone()), self.handler.clone())
        }
    }

    #[test]
    fn proper_shutdown() {
	    struct MyHandler {
			did_substream_attempt: bool,
			inbound_closed: bool,
			substream_attempt_cancelled: bool,
			shutdown_called: bool,
		}
		impl<T> NodeHandler<T> for MyHandler {
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
			fn inject_event(&mut self, _inevent: Self::InEvent) { }
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

		impl Drop for MyHandler {
			fn drop(&mut self) {
				if self.did_substream_attempt {
					assert!(self.shutdown_called);
				}
			}
		}

        // Test that `shutdown()` is properly called on the handler once a node stops.
        let mut muxer = DummyMuxer::new();
        muxer.set_inbound_connection_state(DummyConnectionState::Closed);
        muxer.set_outbound_connection_state(DummyConnectionState::Closed);
        let handled = HandledNode::new(muxer, future::empty(), MyHandler {
			did_substream_attempt: false,
			inbound_closed: false,
			substream_attempt_cancelled: false,
			shutdown_called: false,
		});

        current_thread::Runtime::new().unwrap().block_on(handled.for_each(|_| Ok(()))).unwrap();
    }

    #[test]
    fn new_works() {
        let addr_fut = future::ok("/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr"));
        let muxer = DummyMuxer::new();
        let handler = Handler::default();
        let handled = HandledNode::new(muxer, addr_fut, handler);
        assert!(handled.node.is_some());
    }

    #[test]
    fn can_inject_event() {
        let mut handled = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Closed)
            .handled_node();

        let event = Event::Accept;
        handled.inject_event(event);
        assert!(handled.events().contains(&event)); // todo: maybe just `did_see_event(&event) -> bool`?
    }

    #[test]
    fn knows_if_inbound_is_closed() {
        let mut handled = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Closed)
			.with_handler_state(HandlerState::Ready(None)) // or we get into an infinite loop
            .handled_node();
        handled.poll().expect("poll failed");
        assert!(handled.is_inbound_closed())
    }

    #[test]
    fn knows_if_outbound_is_closed() {
        let mut handled = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Pending)
            .with_muxer_outbound_state(DummyConnectionState::Closed)
			.with_handler_state(HandlerState::Ready(None)) // or we get into an infinite loop
            .handled_node();

		handled.node.as_mut().map(|ns| ns.open_substream(()));
        handled.poll().expect("poll failed");
        assert!(handled.is_outbound_closed());
    }

    #[test]
    fn is_shutting_down() {
        // with in/outbound NodeStreams closed we shut down
        let mut handled = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Closed)
            .with_muxer_outbound_state(DummyConnectionState::Closed)
            .handled_node();
		handled.node.as_mut().map(|ns| ns.open_substream(())); // without an outbound substream we never poll_outbound
        handled.poll().expect("poll failed");
        assert!(handled.is_shutting_down());

        // with open or Async::Ready(None) streams we check the handler
        let mut handled = TestBuilder::new()
            .with_muxer_inbound_state(DummyConnectionState::Pending)
            .with_muxer_outbound_state(DummyConnectionState::Pending)
			.with_handler_state(HandlerState::Ready(None)) // or we end up in an infinite loop
            .handled_node();

        handled.poll().expect("poll failed");

        // still open for business
        assert!(!handled.is_shutting_down());
    }

	#[test]
	fn poll_with_unready_node_stream_polls_handler() {
		let mut handled = TestBuilder::new()
			// make NodeStream return NotReady
			.with_muxer_inbound_state(DummyConnectionState::Pending)
			// make Handler return return Ready(None) so we break the infinite loop
			.with_handler_state(HandlerState::Ready(None))
			.handled_node();
		assert_matches!(handled.poll(), Ok(Async::Ready(None)));
	}

	#[test]
	fn poll_with_unready_node_stream_and_handler_emits_custom_event() {
		let expected_event = Some(NodeHandlerEvent::Custom(Event::Custom));
		let mut handled = TestBuilder::new()
			// make NodeStream return NotReady
			.with_muxer_inbound_state(DummyConnectionState::Pending)
			// make Handler return return Ready(Some(…))
			.with_handler_state(HandlerState::Ready(expected_event))
			.handled_node();

		assert_matches!(handled.poll(), Ok(Async::Ready(Some(event))) => {
			assert_matches!(event, Event::Custom)
		});
	}

	#[test]
	fn poll_with_unready_node_stream_and_handler_emits_outbound() {
		let expected_event = Some(NodeHandlerEvent::OutboundSubstreamRequest(()));
		let mut handled = TestBuilder::new()
			// make NodeStream return NotReady
			.with_muxer_inbound_state(DummyConnectionState::Pending)
			// make Handler return return Ready(Some(…))
			.with_handler_state(HandlerState::Ready(expected_event))
			.handled_node();

		// assert_matches!(handled.poll(), Ok(Async::Ready(Some(event))) => {
		// 	assert_matches!(event, Event::Custom)
		// });

	}
}
