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

// Implementor notes
// =================
//
// In order to minimize the risk of bugs in higher-level code, we want to avoid as much as
// possible having a racy API. The behaviour of methods should be well-defined and predictable.
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
pub struct NodeStream<TMuxer, TUserData>
where
    TMuxer: muxing::StreamMuxer,
{
    /// The muxer used to manage substreams.
    muxer: Arc<TMuxer>,
    /// Tracks the state of the muxers inbound direction.
    inbound_state: StreamState,
    /// Tracks the state of the muxers outbound direction.
    outbound_state: StreamState,
    /// List of substreams we are currently opening.
    outbound_substreams: SmallVec<[(TUserData, TMuxer::OutboundSubstream); 8]>,
}

/// A successfully opened substream.
pub type Substream<TMuxer> = muxing::SubstreamRef<Arc<TMuxer>>;

// Track state of stream muxer per direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamState {
    // direction is open
    Open,
    // direction is shutting down
    Shutdown,
    // direction has shutdown and is flushing
    Flush,
    // direction is closed
    Closed
}

/// Event that can happen on the `NodeStream`.
#[derive(Debug)]
pub enum NodeEvent<TMuxer, TUserData>
where
    TMuxer: muxing::StreamMuxer,
{
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

impl<TMuxer, TUserData> NodeStream<TMuxer, TUserData>
where
    TMuxer: muxing::StreamMuxer,
{
    /// Creates a new node events stream.
    #[inline]
    pub fn new(muxer: TMuxer) -> Self {
        NodeStream {
            muxer: Arc::new(muxer),
            inbound_state: StreamState::Open,
            outbound_state: StreamState::Open,
            outbound_substreams: SmallVec::new(),
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
        if self.outbound_state != StreamState::Open {
            return Err(user_data);
        }

        let raw = self.muxer.open_outbound();
        self.outbound_substreams.push((user_data, raw));

        Ok(())
    }

    /// Returns true if the inbound channel of the muxer is open.
    ///
    /// If `true` is returned, more inbound substream will be produced.
    #[inline]
    pub fn is_inbound_open(&self) -> bool {
        self.inbound_state == StreamState::Open
    }

    /// Returns true if the outbound channel of the muxer is open.
    ///
    /// If `true` is returned, more outbound substream can be opened. Otherwise, calling
    /// `open_substream` will return an `Err`.
    #[inline]
    pub fn is_outbound_open(&self) -> bool {
        self.outbound_state == StreamState::Open
    }

    /// Destroys the node stream and returns all the pending outbound substreams.
    pub fn close(mut self) -> Vec<TUserData> {
        self.cancel_outgoing()
    }

    /// Destroys all outbound streams and returns the corresponding user data.
    pub fn cancel_outgoing(&mut self) -> Vec<TUserData> {
        let mut out = Vec::with_capacity(self.outbound_substreams.len());
        for (user_data, outbound) in self.outbound_substreams.drain() {
            out.push(user_data);
            self.muxer.destroy_outbound(outbound);
        }
        out
    }

    /// Trigger node shutdown.
    ///
    /// After this, `NodeStream::poll` will eventually produce `None`, when both endpoints are
    /// closed.
    pub fn shutdown_all(&mut self) {
        if self.inbound_state == StreamState::Open {
            self.inbound_state = StreamState::Shutdown
        }
        if self.outbound_state == StreamState::Open {
            self.outbound_state = StreamState::Shutdown
        }
    }

    // If in progress, drive this node's stream muxer shutdown to completion.
    fn poll_shutdown(&mut self) -> Poll<(), IoError> {
        use self::StreamState::*;
        loop {
            match (self.inbound_state, self.outbound_state) {
                (Open, Open) | (Open, Closed) | (Closed, Open) | (Closed, Closed) => {
                    return Ok(Async::Ready(()))
                }
                (Shutdown, Shutdown) => {
                    if let Async::Ready(()) = self.muxer.shutdown(muxing::Shutdown::All)? {
                        self.inbound_state = StreamState::Flush;
                        self.outbound_state = StreamState::Flush;
                        continue
                    }
                    return Ok(Async::NotReady)
                }
                (Shutdown, _) => {
                    if let Async::Ready(()) = self.muxer.shutdown(muxing::Shutdown::Inbound)? {
                        self.inbound_state = StreamState::Flush;
                        continue
                    }
                    return Ok(Async::NotReady)
                }
                (_, Shutdown) => {
                    if let Async::Ready(()) = self.muxer.shutdown(muxing::Shutdown::Outbound)? {
                        self.outbound_state = StreamState::Flush;
                        continue
                    }
                    return Ok(Async::NotReady)
                }
                (Flush, Open) => {
                    if let Async::Ready(()) = self.muxer.flush_all()? {
                        self.inbound_state = StreamState::Closed;
                        continue
                    }
                    return Ok(Async::NotReady)
                }
                (Open, Flush) => {
                    if let Async::Ready(()) = self.muxer.flush_all()? {
                        self.outbound_state = StreamState::Closed;
                        continue
                    }
                    return Ok(Async::NotReady)
                }
                (Flush, Flush) | (Flush, Closed) | (Closed, Flush) => {
                    if let Async::Ready(()) = self.muxer.flush_all()? {
                        self.inbound_state = StreamState::Closed;
                        self.outbound_state = StreamState::Closed;
                        continue
                    }
                    return Ok(Async::NotReady)
                }
            }
        }
    }
}

impl<TMuxer, TUserData> Stream for NodeStream<TMuxer, TUserData>
where
    TMuxer: muxing::StreamMuxer,
{
    type Item = NodeEvent<TMuxer, TUserData>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Drive the shutdown process, if any.
        if self.poll_shutdown()?.is_not_ready() {
            return Ok(Async::NotReady)
        }

        // Polling inbound substream.
        if self.inbound_state == StreamState::Open {
            match self.muxer.poll_inbound()? {
                Async::Ready(Some(substream)) => {
                    let substream = muxing::substream_from_ref(self.muxer.clone(), substream);
                    return Ok(Async::Ready(Some(NodeEvent::InboundSubstream {
                        substream,
                    })));
                }
                Async::Ready(None) => {
                    self.inbound_state = StreamState::Closed;
                    return Ok(Async::Ready(Some(NodeEvent::InboundClosed)));
                }
                Async::NotReady => {}
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
                    self.outbound_state = StreamState::Closed;
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

        // Closing the node if there's no way we can do anything more.
        if self.inbound_state == StreamState::Closed
            && self.outbound_state == StreamState::Closed
            && self.outbound_substreams.is_empty()
        {
            return Ok(Async::Ready(None))
        }

        // Nothing happened. Register our task to be notified and return.
        Ok(Async::NotReady)
    }
}

impl<TMuxer, TUserData> fmt::Debug for NodeStream<TMuxer, TUserData>
where
    TMuxer: muxing::StreamMuxer,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct("NodeStream")
            .field("inbound_state", &self.inbound_state)
            .field("outbound_state", &self.outbound_state)
            .field("outbound_substreams", &self.outbound_substreams.len())
            .finish()
    }
}

impl<TMuxer, TUserData> Drop for NodeStream<TMuxer, TUserData>
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
    use super::NodeStream;
    use futures::prelude::*;
    use tokio_mock_task::MockTask;
    use super::NodeEvent;
    use tests::dummy_muxer::{DummyMuxer, DummyConnectionState};

    fn build_node_stream() -> NodeStream<DummyMuxer, Vec<u8>> {
        let muxer = DummyMuxer::new();
        NodeStream::<_, Vec<u8>>::new(muxer)
    }

    #[test]
    fn can_open_outbound_substreams_until_an_outbound_channel_is_closed() {
        let mut muxer = DummyMuxer::new();
        muxer.set_outbound_connection_state(DummyConnectionState::Closed);
        let mut ns = NodeStream::<_, Vec<u8>>::new(muxer);

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

        // Opening a second substream fails because `outbound_state` is no longer open.
        assert_matches!(ns.open_substream(vec![22]), Err(user_data) => {
            assert_eq!(user_data, vec![22]);
        });
    }

    #[test]
    fn query_inbound_outbound_state() {
        let ns = build_node_stream();
        assert!(ns.is_inbound_open());
        assert!(ns.is_outbound_open());
    }

    #[test]
    fn query_inbound_state() {
        let mut muxer = DummyMuxer::new();
        muxer.set_inbound_connection_state(DummyConnectionState::Closed);
        let mut ns = NodeStream::<_, Vec<u8>>::new(muxer);

        assert_matches!(ns.poll(), Ok(Async::Ready(Some(node_event))) => {
            assert_matches!(node_event, NodeEvent::InboundClosed)
        });

        assert!(!ns.is_inbound_open());
    }

    #[test]
    fn query_outbound_state() {
        let mut muxer = DummyMuxer::new();
        muxer.set_outbound_connection_state(DummyConnectionState::Closed);
        let mut ns = NodeStream::<_, Vec<u8>>::new(muxer);

        assert!(ns.is_outbound_open());

        ns.open_substream(vec![1]).unwrap();
        let poll_result = ns.poll();

        assert_matches!(poll_result, Ok(Async::Ready(Some(node_event))) => {
            assert_matches!(node_event, NodeEvent::OutboundClosed{user_data} => {
                assert_eq!(user_data, vec![1])
            })
        });

        assert!(!ns.is_outbound_open(), "outbound connection should be closed after polling");
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
            let mut muxer = DummyMuxer::new();
            // ensure muxer.poll_inbound() returns Async::NotReady
            muxer.set_inbound_connection_state(DummyConnectionState::Pending);
            // ensure muxer.poll_outbound() returns Async::NotReady
            muxer.set_outbound_connection_state(DummyConnectionState::Pending);
            let mut ns = NodeStream::<_, Vec<u8>>::new(muxer);

            assert_matches!(ns.poll(), Ok(Async::NotReady));
        });
    }

    #[test]
    fn poll_closes_the_node_stream_when_no_more_work_can_be_done() {
        let mut muxer = DummyMuxer::new();
        // ensure muxer.poll_inbound() returns Async::Ready(None)
        muxer.set_inbound_connection_state(DummyConnectionState::Closed);
        // ensure muxer.poll_outbound() returns Async::Ready(None)
        muxer.set_outbound_connection_state(DummyConnectionState::Closed);
        let mut ns = NodeStream::<_, Vec<u8>>::new(muxer);
        ns.open_substream(vec![]).unwrap();
        ns.poll().unwrap(); // poll_inbound()
        ns.poll().unwrap(); // poll_outbound()
        ns.poll().unwrap(); // resolve the address
        // Nothing more to do, the NodeStream should be closed
        assert_matches!(ns.poll(), Ok(Async::Ready(None)));
    }

    #[test]
    fn poll_sets_up_substreams_yielding_them_in_reverse_order() {
        let mut muxer = DummyMuxer::new();
        // ensure muxer.poll_inbound() returns Async::Ready(None)
        muxer.set_inbound_connection_state(DummyConnectionState::Closed);
        // ensure muxer.poll_outbound() returns Async::Ready(Some(substream))
        muxer.set_outbound_connection_state(DummyConnectionState::Opened);
        let mut ns = NodeStream::<_, Vec<u8>>::new(muxer);
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
        let mut muxer = DummyMuxer::new();
        // ensure muxer.poll_inbound() returns Async::Ready(None)
        muxer.set_inbound_connection_state(DummyConnectionState::Closed);
        // ensure muxer.poll_outbound() returns Async::NotReady
        muxer.set_outbound_connection_state(DummyConnectionState::Pending);
        let mut ns = NodeStream::<_, Vec<u8>>::new(muxer);
        ns.open_substream(vec![1]).unwrap();
        ns.poll().unwrap(); // poll past inbound
        ns.poll().unwrap(); // poll outbound
        assert!(ns.is_outbound_open());
        assert!(format!("{:?}", ns).contains("outbound_substreams: 1"));
    }

    #[test]
    fn poll_returns_incoming_substream() {
        let mut muxer = DummyMuxer::new();
        // ensure muxer.poll_inbound() returns Async::Ready(Some(subs))
        muxer.set_inbound_connection_state(DummyConnectionState::Opened);
        let mut ns = NodeStream::<_, Vec<u8>>::new(muxer);
        assert_matches!(ns.poll(), Ok(Async::Ready(Some(node_event))) => {
            assert_matches!(node_event, NodeEvent::InboundSubstream{ substream: _ });
        });
    }
}
