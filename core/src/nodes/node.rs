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

use futures::prelude::*;
use muxing;
use smallvec::SmallVec;
use std::fmt;
use std::io::Error as IoError;
use std::sync::Arc;
use Multiaddr;

// Implementor notes
// =================
//
// In order to minimize the risk of bugs in higher-level code, we want to avoid as much as
// possible having a racy API. The behaviour of methods should be well-defined and predictable.
// As an example, calling the `multiaddr()` method should return `Some` only after a
// `MultiaddrResolved` event has been emitted and never before, even if we technically already
// know the address.
//
// In order to respect this coding practice, we should theoretically provide events such as "data
// incoming on a substream", or "a substream is ready to be written". This would however make the
// API of `NodeStream` really painful to use. Instead, we really want to provide an object that
// implements the `AsyncRead` and `AsyncWrite` traits.
//
// This substream object raises the question of how to keep the `NodeStream` and the various
// substreams in sync without exposing a racy API. The answer is that we don't. The state of the
// node and the state of the substreams are totally detached, and they don't interact with each
// other in any way. Destroying the `NodeStream` doesn't close the substreams, nor is there a
// `close_substreams()` method or a "substream closed" event.

/// Implementation of `Stream` that handles a node.
///
/// The stream will receive substreams and can be used to open new outgoing substreams. Destroying
/// the `NodeStream` will **not** close the existing substreams.
///
/// The stream will close once both the inbound and outbound channels are closed, and no more
/// outbound substream attempt is pending.
pub struct NodeStream<TMuxer, TAddrFut, TUserData>
where
    TMuxer: muxing::StreamMuxer,
{
    /// The muxer used to manage substreams.
    muxer: Arc<TMuxer>,
    /// If true, the inbound side of the muxer has closed earlier and should no longer be polled.
    inbound_finished: bool,
    /// If true, the outbound side of the muxer has closed earlier.
    outbound_finished: bool,
    /// Address of the node ; can be empty if the address hasn't been resolved yet.
    address: Addr<TAddrFut>,
    /// List of substreams we are currently opening.
    outbound_substreams: SmallVec<[(TUserData, TMuxer::OutboundSubstream); 8]>,
}

/// Address of the node.
#[derive(Debug, Clone)]
enum Addr<TAddrFut> {
    /// Future that will resolve the address.
    Future(TAddrFut),
    /// The address is now known.
    Resolved(Multiaddr),
    /// An error happened while resolving the future.
    Errored,
}

/// A successfully opened substream.
pub type Substream<TMuxer> = muxing::SubstreamRef<Arc<TMuxer>>;

/// Event that can happen on the `NodeStream`.
#[derive(Debug)]
pub enum NodeEvent<TMuxer, TUserData>
where
    TMuxer: muxing::StreamMuxer,
{
    /// The multiaddress future of the node has been resolved.
    ///
    /// If this succeeded, after this event has been emitted calling `multiaddr()` will return
    /// `Some`.
    Multiaddr(Result<Multiaddr, IoError>),

    /// A new inbound substream arrived.
    InboundSubstream {
        /// The newly-opened substream.
        substream: Substream<TMuxer>,
    },

    /// An outbound substream has successfully been opened.
    OutboundSubstream {
        /// User data that has been passed to the `open_substream` method.
        user_data: TUserData,
        /// The newly-opened substream.
        substream: Substream<TMuxer>,
    },

    /// An outbound substream couldn't be opened because the muxer is no longer capable of opening
    /// more substreams.
    OutboundClosed {
        /// User data that has been passed to the `open_substream` method.
        user_data: TUserData,
    },

    /// The inbound side of the muxer has been closed. No more inbound substreams will be produced.
    InboundClosed,
}

/// Identifier for a substream being opened.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutboundSubstreamId(usize);

impl<TMuxer, TAddrFut, TUserData> NodeStream<TMuxer, TAddrFut, TUserData>
where
    TMuxer: muxing::StreamMuxer,
    TAddrFut: Future<Item = Multiaddr, Error = IoError>,
{
    /// Creates a new node events stream.
    #[inline]
    pub fn new(muxer: TMuxer, multiaddr_future: TAddrFut) -> Self {
        NodeStream {
            muxer: Arc::new(muxer),
            inbound_finished: false,
            outbound_finished: false,
            address: Addr::Future(multiaddr_future),
            outbound_substreams: SmallVec::new(),
        }
    }

    /// Returns the multiaddress of the node, if already known.
    ///
    /// This method will always return `None` before a successful `Multiaddr` event has been
    /// returned by `poll()`, and will always return `Some` afterwards.
    #[inline]
    pub fn multiaddr(&self) -> Option<&Multiaddr> {
        match self.address {
            Addr::Resolved(ref addr) => Some(addr),
            Addr::Future(_) | Addr::Errored => None,
        }
    }

    /// Starts the process of opening a new outbound substream.
    ///
    /// Returns an error if the outbound side of the muxer is closed.
    ///
    /// After calling this method, polling the stream should eventually produce either an
    /// `OutboundSubstream` event or an `OutboundClosed` event containing the user data that has
    /// been passed to this method.
    pub fn open_substream(&mut self, user_data: TUserData) -> Result<(), TUserData> {
        if self.outbound_finished {
            return Err(user_data);
        }

        let raw = self.muxer.open_outbound();
        self.outbound_substreams.push((user_data, raw));

        Ok(())
    }

    /// Returns true if the inbound channel of the muxer is closed.
    ///
    /// If `true` is returned, then no more inbound substream will be produced.
    #[inline]
    pub fn is_inbound_closed(&self) -> bool {
        self.inbound_finished
    }

    /// Returns true if the outbound channel of the muxer is closed.
    ///
    /// If `true` is returned, then no more outbound substream can be opened. Calling
    /// `open_substream` will return an `Err`.
    #[inline]
    pub fn is_outbound_closed(&self) -> bool {
        self.outbound_finished
    }

    /// Destroys the node stream and returns all the pending outbound substreams.
    pub fn close(mut self) -> Vec<TUserData> {
        let mut out = Vec::with_capacity(self.outbound_substreams.len());
        for (user_data, outbound) in self.outbound_substreams.drain() {
            out.push(user_data);
            self.muxer.destroy_outbound(outbound);
        }
        out
    }
}

impl<TMuxer, TAddrFut, TUserData> Stream for NodeStream<TMuxer, TAddrFut, TUserData>
where
    TMuxer: muxing::StreamMuxer,
    TAddrFut: Future<Item = Multiaddr, Error = IoError>,
{
    type Item = NodeEvent<TMuxer, TUserData>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Polling inbound substream.
        if !self.inbound_finished {
            match self.muxer.poll_inbound() {
                Ok(Async::Ready(Some(substream))) => {
                    let substream = muxing::substream_from_ref(self.muxer.clone(), substream);
                    return Ok(Async::Ready(Some(NodeEvent::InboundSubstream {
                        substream,
                    })));
                }
                Ok(Async::Ready(None)) => {
                    self.inbound_finished = true;
                    return Ok(Async::Ready(Some(NodeEvent::InboundClosed)));
                }
                Ok(Async::NotReady) => {}
                Err(err) => return Err(err),
            }
        }

        // Polling outbound substreams.
        // We remove each element from `outbound_substreams` one by one and add them back.
        for n in (0..self.outbound_substreams.len()).rev() {
            let (user_data, mut outbound) = self.outbound_substreams.swap_remove(n);
            match self.muxer.poll_outbound(&mut outbound) {
                Ok(Async::Ready(Some(substream))) => {
                    let substream = muxing::substream_from_ref(self.muxer.clone(), substream);
                    self.muxer.destroy_outbound(outbound);
                    return Ok(Async::Ready(Some(NodeEvent::OutboundSubstream {
                        user_data,
                        substream,
                    })));
                }
                Ok(Async::Ready(None)) => {
                    self.outbound_finished = true;
                    self.muxer.destroy_outbound(outbound);
                    return Ok(Async::Ready(Some(NodeEvent::OutboundClosed { user_data })));
                }
                Ok(Async::NotReady) => {
                    self.outbound_substreams.push((user_data, outbound));
                }
                Err(err) => {
                    self.muxer.destroy_outbound(outbound);
                    return Err(err);
                }
            }
        }

        // Check whether the multiaddress is resolved.
        {
            let poll = match self.address {
                Addr::Future(ref mut fut) => Some(fut.poll()),
                Addr::Resolved(_) | Addr::Errored => None,
            };

            match poll {
                Some(Ok(Async::NotReady)) | None => {}
                Some(Ok(Async::Ready(addr))) => {
                    self.address = Addr::Resolved(addr.clone());
                    return Ok(Async::Ready(Some(NodeEvent::Multiaddr(Ok(addr)))));
                }
                Some(Err(err)) => {
                    self.address = Addr::Errored;
                    return Ok(Async::Ready(Some(NodeEvent::Multiaddr(Err(err)))));
                }
            }
        }

        // Closing the node if there's no way we can do anything more.
        if self.inbound_finished && self.outbound_finished && self.outbound_substreams.is_empty() {
            return Ok(Async::Ready(None));
        }

        // Nothing happened. Register our task to be notified and return.
        Ok(Async::NotReady)
    }
}

impl<TMuxer, TAddrFut, TUserData> fmt::Debug for NodeStream<TMuxer, TAddrFut, TUserData>
where
    TMuxer: muxing::StreamMuxer,
    TAddrFut: Future<Item = Multiaddr, Error = IoError>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct("NodeStream")
            .field("address", &self.multiaddr())
            .field("inbound_finished", &self.inbound_finished)
            .field("outbound_finished", &self.outbound_finished)
            .field("outbound_substreams", &self.outbound_substreams.len())
            .finish()
    }
}

impl<TMuxer, TAddrFut, TUserData> Drop for NodeStream<TMuxer, TAddrFut, TUserData>
where
    TMuxer: muxing::StreamMuxer,
{
    fn drop(&mut self) {
        // The substreams that were produced will continue to work, as the muxer is held in an Arc.
        // However we will no longer process any further inbound or outbound substream, and we
        // therefore close everything.
        for (_, outbound) in self.outbound_substreams.drain() {
            self.muxer.destroy_outbound(outbound);
        }
        if !self.inbound_finished {
            self.muxer.close_inbound();
        }
        if !self.outbound_finished {
            self.muxer.close_outbound();
        }
    }
}

// TODO:
/*impl<TTrans> fmt::Debug for NodeEvent<TTrans>
where TTrans: Transport,
      <TTrans::Listener as Stream>::Error: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            NodeEvent::Incoming { ref listen_addr, .. } => {
                f.debug_struct("NodeEvent::Incoming")
                    .field("listen_addr", listen_addr)
                    .finish()
            },
            NodeEvent::Closed { ref listen_addr, .. } => {
                f.debug_struct("NodeEvent::Closed")
                    .field("listen_addr", listen_addr)
                    .finish()
            },
            NodeEvent::Error { ref listen_addr, ref error, .. } => {
                f.debug_struct("NodeEvent::Error")
                    .field("listen_addr", listen_addr)
                    .field("error", error)
                    .finish()
            },
        }
    }
}*/

#[cfg(test)]
mod node_stream {
    use multiaddr::Multiaddr;
    use super::NodeStream;
    use futures::{future::self, prelude::*, Future};
    use tokio_mock_task::MockTask;
    use super::NodeEvent;
    use tests::dummy_muxer::{DummyMuxer, DummyConnectionState};
    use std::io::Error as IoError;


    fn build_node_stream() -> NodeStream<DummyMuxer, impl Future<Item=Multiaddr, Error=IoError>, Vec<u8>> {
        let addr = future::ok("/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad maddr"));
        let muxer = DummyMuxer::new();
        NodeStream::<_, _, Vec<u8>>::new(muxer, addr)
    }

    #[test]
    fn multiaddr_is_available_once_polled() {
        let mut node_stream = build_node_stream();
        assert!(node_stream.multiaddr().is_none());
        match node_stream.poll() {
            Ok(Async::Ready(Some(NodeEvent::Multiaddr(Ok(addr))))) => {
                assert_eq!(addr.to_string(), "/ip4/127.0.0.1/tcp/1234")
            }
            _ => panic!("unexpected poll return value" )
        }
        assert!(node_stream.multiaddr().is_some());
    }

    #[test]
    fn can_open_outbound_substreams_until_an_outbound_channel_is_closed() {
        let addr = future::ok("/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad maddr"));
        let mut muxer = DummyMuxer::new();
        muxer.set_outbound_connection_state(DummyConnectionState::Closed);
        let mut ns = NodeStream::<_, _, Vec<u8>>::new(muxer, addr);

        // open first substream works
        assert!(ns.open_substream(vec![1,2,3]).is_ok());

        // Given the state we set on the DummyMuxer, once we poll() we'll get an
        // `OutboundClosed` which will make subsequent calls to `open_substream` fail
        let out = ns.poll();
        assert_matches!(out, Ok(Async::Ready(Some(node_event))) => {
            assert_matches!(node_event, NodeEvent::OutboundClosed{user_data} => {
                assert_eq!(user_data, vec![1,2,3])
            })
        });

        // Opening a second substream fails because `outbound_finished` is now true
        assert_matches!(ns.open_substream(vec![22]), Err(user_data) => {
            assert_eq!(user_data, vec![22]);
        });
    }

    #[test]
    fn query_inbound_outbound_state() {
        let ns = build_node_stream();
        assert_eq!(ns.is_inbound_closed(), false);
        assert_eq!(ns.is_outbound_closed(), false);
    }

    #[test]
    fn query_inbound_state() {
        let addr = future::ok("/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad maddr"));
        let mut muxer = DummyMuxer::new();
        muxer.set_inbound_connection_state(DummyConnectionState::Closed);
        let mut ns = NodeStream::<_, _, Vec<u8>>::new(muxer, addr);

        assert_matches!(ns.poll(), Ok(Async::Ready(Some(node_event))) => {
            assert_matches!(node_event, NodeEvent::InboundClosed)
        });

        assert_eq!(ns.is_inbound_closed(), true);
    }

    #[test]
    fn query_outbound_state() {
        let addr = future::ok("/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr"));
        let mut muxer = DummyMuxer::new();
        muxer.set_outbound_connection_state(DummyConnectionState::Closed);
        let mut ns = NodeStream::<_, _, Vec<u8>>::new(muxer, addr);

        assert_eq!(ns.is_outbound_closed(), false);

        ns.open_substream(vec![1]).unwrap();
        let poll_result = ns.poll();

        assert_matches!(poll_result, Ok(Async::Ready(Some(node_event))) => {
            assert_matches!(node_event, NodeEvent::OutboundClosed{user_data} => {
                assert_eq!(user_data, vec![1])
            })
        });

        assert_eq!(ns.is_outbound_closed(), true, "outbound connection should be closed after polling");
    }

    #[test]
    fn closing_a_node_stream_destroys_substreams_and_returns_submitted_user_data() {
        let mut ns = build_node_stream();
        ns.open_substream(vec![2]).unwrap();
        ns.open_substream(vec![3]).unwrap();
        ns.open_substream(vec![5]).unwrap();
        let user_data_submitted = ns.close();
        assert_eq!(user_data_submitted, vec![
            vec![2], vec![3], vec![5]
        ]);
    }

    #[test]
    fn poll_returns_not_ready_when_there_is_nothing_to_do() {
        let mut task = MockTask::new();
        task.enter(|| {
            // ensure the address never resolves
            let addr = future::empty();
            let mut muxer = DummyMuxer::new();
            // ensure muxer.poll_inbound() returns Async::NotReady
            muxer.set_inbound_connection_state(DummyConnectionState::Pending);
            // ensure muxer.poll_outbound() returns Async::NotReady
            muxer.set_outbound_connection_state(DummyConnectionState::Pending);
            let mut ns = NodeStream::<_, _, Vec<u8>>::new(muxer, addr);

            assert_matches!(ns.poll(), Ok(Async::NotReady));
        });
    }

    #[test]
    fn poll_closes_the_node_stream_when_no_more_work_can_be_done() {
        let addr = future::ok("/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr"));
        let mut muxer = DummyMuxer::new();
        // ensure muxer.poll_inbound() returns Async::Ready(None)
        muxer.set_inbound_connection_state(DummyConnectionState::Closed);
        // ensure muxer.poll_outbound() returns Async::Ready(None)
        muxer.set_outbound_connection_state(DummyConnectionState::Closed);
        let mut ns = NodeStream::<_, _, Vec<u8>>::new(muxer, addr);
        ns.open_substream(vec![]).unwrap();
        ns.poll().unwrap(); // poll_inbound()
        ns.poll().unwrap(); // poll_outbound()
        ns.poll().unwrap(); // resolve the address
        // Nothing more to do, the NodeStream should be closed
        assert_matches!(ns.poll(), Ok(Async::Ready(None)));
    }

    #[test]
    fn poll_resolves_the_address() {
        let addr = future::ok("/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr"));
        let mut muxer = DummyMuxer::new();
        // ensure muxer.poll_inbound() returns Async::Ready(None)
        muxer.set_inbound_connection_state(DummyConnectionState::Closed);
        // ensure muxer.poll_outbound() returns Async::Ready(None)
        muxer.set_outbound_connection_state(DummyConnectionState::Closed);
        let mut ns = NodeStream::<_, _, Vec<u8>>::new(muxer, addr);
        ns.open_substream(vec![]).unwrap();
        ns.poll().unwrap(); // poll_inbound()
        ns.poll().unwrap(); // poll_outbound()
        assert_matches!(ns.poll(), Ok(Async::Ready(Some(node_event))) => {
            assert_matches!(node_event, NodeEvent::Multiaddr(Ok(_)))
        });
    }

    #[test]
    fn poll_sets_up_substreams_yielding_them_in_reverse_order() {
        let addr = future::ok("/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr"));
        let mut muxer = DummyMuxer::new();
        // ensure muxer.poll_inbound() returns Async::Ready(None)
        muxer.set_inbound_connection_state(DummyConnectionState::Closed);
        // ensure muxer.poll_outbound() returns Async::Ready(Some(substream))
        muxer.set_outbound_connection_state(DummyConnectionState::Opened);
        let mut ns = NodeStream::<_, _, Vec<u8>>::new(muxer, addr);
        ns.open_substream(vec![1]).unwrap();
        ns.open_substream(vec![2]).unwrap();
        ns.poll().unwrap(); // poll_inbound()

        // poll() sets up second outbound substream
        assert_matches!(ns.poll(), Ok(Async::Ready(Some(node_event))) => {
            assert_matches!(node_event, NodeEvent::OutboundSubstream{ user_data, substream:_ } => {
                assert_eq!(user_data, vec![2]);
            })
        });
        // Next poll() sets up first outbound substream
        assert_matches!(ns.poll(), Ok(Async::Ready(Some(node_event))) => {
            assert_matches!(node_event, NodeEvent::OutboundSubstream{ user_data, substream: _ } => {
                assert_eq!(user_data, vec![1]);
            })
        });
    }

    #[test]
    fn poll_keeps_outbound_substreams_when_the_outgoing_connection_is_not_ready() {
        let addr = future::ok("/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr"));
        let mut muxer = DummyMuxer::new();
        // ensure muxer.poll_inbound() returns Async::Ready(None)
        muxer.set_inbound_connection_state(DummyConnectionState::Closed);
        // ensure muxer.poll_outbound() returns Async::NotReady
        muxer.set_outbound_connection_state(DummyConnectionState::Pending);
        let mut ns = NodeStream::<_, _, Vec<u8>>::new(muxer, addr);
        ns.open_substream(vec![1]).unwrap();
        ns.poll().unwrap(); // poll past inbound
        ns.poll().unwrap(); // poll outbound
        assert_eq!(ns.is_outbound_closed(), false);
        assert!(format!("{:?}", ns).contains("outbound_substreams: 1"));
    }

    #[test]
    fn poll_returns_incoming_substream() {
        let addr = future::ok("/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr"));
        let mut muxer = DummyMuxer::new();
        // ensure muxer.poll_inbound() returns Async::Ready(Some(subs))
        muxer.set_inbound_connection_state(DummyConnectionState::Opened);
        let mut ns = NodeStream::<_, _, Vec<u8>>::new(muxer, addr);
        assert_matches!(ns.poll(), Ok(Async::Ready(Some(node_event))) => {
            assert_matches!(node_event, NodeEvent::InboundSubstream{ substream: _ });
        });
    }
}
