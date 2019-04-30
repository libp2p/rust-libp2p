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
use crate::muxing;
use smallvec::SmallVec;
use std::fmt;
use std::io::Error as IoError;
use std::sync::Arc;

// Implementation notes
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
// substreams in sync without exposing a racy API. The answer is that the `NodeStream` holds
// ownership of the connection. Shutting node the `NodeStream` or destroying it will close all the
// existing substreams. The user of the `NodeStream` should be aware of that.

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
    /// List of substreams we are currently opening.
    outbound_substreams: SmallVec<[(TUserData, TMuxer::OutboundSubstream); 8]>,
}

/// Future that signals the remote that we have closed the connection.
pub struct Close<TMuxer> {
    /// Muxer to close.
    muxer: Arc<TMuxer>,
}

/// A successfully opened substream.
pub type Substream<TMuxer> = muxing::SubstreamRef<Arc<TMuxer>>;

/// Event that can happen on the `NodeStream`.
pub enum NodeEvent<TMuxer, TUserData>
where
    TMuxer: muxing::StreamMuxer,
{
    /// A new inbound substream arrived.
    InboundSubstream {
        /// The newly-opened substream. Will return EOF of an error if the `NodeStream` is
        /// destroyed or `close_graceful` is called.
        substream: Substream<TMuxer>,
    },

    /// An outbound substream has successfully been opened.
    OutboundSubstream {
        /// User data that has been passed to the `open_substream` method.
        user_data: TUserData,
        /// The newly-opened substream. Will return EOF of an error if the `NodeStream` is
        /// destroyed or `close_graceful` is called.
        substream: Substream<TMuxer>,
    },
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
            outbound_substreams: SmallVec::new(),
        }
    }

    /// Starts the process of opening a new outbound substream.
    ///
    /// After calling this method, polling the stream should eventually produce either an
    /// `OutboundSubstream` event or an `OutboundClosed` event containing the user data that has
    /// been passed to this method.
    pub fn open_substream(&mut self, user_data: TUserData) {
        let raw = self.muxer.open_outbound();
        self.outbound_substreams.push((user_data, raw));
    }

    /// Returns `true` if the remote has shown any sign of activity after the muxer has been open.
    ///
    /// See `StreamMuxer::is_remote_acknowledged`.
    pub fn is_remote_acknowledged(&self) -> bool {
        self.muxer.is_remote_acknowledged()
    }

    /// Destroys the node stream and returns all the pending outbound substreams, plus an object
    /// that signals the remote that we shut down the connection.
    #[must_use]
    pub fn close(mut self) -> (Close<TMuxer>, Vec<TUserData>) {
        let substreams = self.cancel_outgoing();
        let close = Close { muxer: self.muxer.clone() };
        (close, substreams)
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

    /// Provides an API similar to `Future`.
    pub fn poll(&mut self) -> Poll<NodeEvent<TMuxer, TUserData>, IoError> {
        // Polling inbound substream.
        match self.muxer.poll_inbound().map_err(|e| e.into())? {
            Async::Ready(substream) => {
                let substream = muxing::substream_from_ref(self.muxer.clone(), substream);
                return Ok(Async::Ready(NodeEvent::InboundSubstream {
                    substream,
                }));
            }
            Async::NotReady => {}
        }

        // Polling outbound substreams.
        // We remove each element from `outbound_substreams` one by one and add them back.
        for n in (0..self.outbound_substreams.len()).rev() {
            let (user_data, mut outbound) = self.outbound_substreams.swap_remove(n);
            match self.muxer.poll_outbound(&mut outbound) {
                Ok(Async::Ready(substream)) => {
                    let substream = muxing::substream_from_ref(self.muxer.clone(), substream);
                    self.muxer.destroy_outbound(outbound);
                    return Ok(Async::Ready(NodeEvent::OutboundSubstream {
                        user_data,
                        substream,
                    }));
                }
                Ok(Async::NotReady) => {
                    self.outbound_substreams.push((user_data, outbound));
                }
                Err(err) => {
                    self.muxer.destroy_outbound(outbound);
                    return Err(err.into());
                }
            }
        }

        // Nothing happened. Register our task to be notified and return.
        Ok(Async::NotReady)
    }
}

impl<TMuxer, TUserData> fmt::Debug for NodeStream<TMuxer, TUserData>
where
    TMuxer: muxing::StreamMuxer,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("NodeStream")
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

impl<TMuxer> Future for Close<TMuxer>
where
    TMuxer: muxing::StreamMuxer,
{
    type Item = ();
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.muxer.close().map_err(|e| e.into())
    }
}

impl<TMuxer> fmt::Debug for Close<TMuxer>
where
    TMuxer: muxing::StreamMuxer,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("Close")
            .finish()
    }
}

impl<TMuxer, TUserData> fmt::Debug for NodeEvent<TMuxer, TUserData>
where
    TMuxer: muxing::StreamMuxer,
    TMuxer::Substream: fmt::Debug,
    TUserData: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeEvent::InboundSubstream { substream } => {
                f.debug_struct("NodeEvent::OutboundClosed")
                    .field("substream", substream)
                    .finish()
            },
            NodeEvent::OutboundSubstream { user_data, substream } => {
                f.debug_struct("NodeEvent::OutboundSubstream")
                    .field("user_data", user_data)
                    .field("substream", substream)
                    .finish()
            },
        }
    }
}

#[cfg(test)]
mod node_stream {
    use super::{NodeEvent, NodeStream};
    use crate::tests::dummy_muxer::{DummyMuxer, DummyConnectionState};
    use assert_matches::assert_matches;
    use futures::prelude::*;
    use tokio_mock_task::MockTask;

    fn build_node_stream() -> NodeStream<DummyMuxer, Vec<u8>> {
        let muxer = DummyMuxer::new();
        NodeStream::<_, Vec<u8>>::new(muxer)
    }

    #[test]
    fn closing_a_node_stream_destroys_substreams_and_returns_submitted_user_data() {
        let mut ns = build_node_stream();
        ns.open_substream(vec![2]);
        ns.open_substream(vec![3]);
        ns.open_substream(vec![5]);
        let user_data_submitted = ns.close();
        assert_eq!(user_data_submitted.1, vec![
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
    fn poll_keeps_outbound_substreams_when_the_outgoing_connection_is_not_ready() {
        let mut muxer = DummyMuxer::new();
        // ensure muxer.poll_inbound() returns Async::NotReady
        muxer.set_inbound_connection_state(DummyConnectionState::Pending);
        // ensure muxer.poll_outbound() returns Async::NotReady
        muxer.set_outbound_connection_state(DummyConnectionState::Pending);
        let mut ns = NodeStream::<_, Vec<u8>>::new(muxer);
        ns.open_substream(vec![1]);
        ns.poll().unwrap(); // poll past inbound
        ns.poll().unwrap(); // poll outbound
        assert!(format!("{:?}", ns).contains("outbound_substreams: 1"));
    }

    #[test]
    fn poll_returns_incoming_substream() {
        let mut muxer = DummyMuxer::new();
        // ensure muxer.poll_inbound() returns Async::Ready(subs)
        muxer.set_inbound_connection_state(DummyConnectionState::Opened);
        let mut ns = NodeStream::<_, Vec<u8>>::new(muxer);
        assert_matches!(ns.poll(), Ok(Async::Ready(node_event)) => {
            assert_matches!(node_event, NodeEvent::InboundSubstream{ substream: _ });
        });
    }
}
