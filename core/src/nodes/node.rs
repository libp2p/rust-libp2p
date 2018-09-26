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

use futures::{prelude::*, task};
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
    /// If true, the inbound side of the muxer has closed earlier and should no longer be polled.
    inbound_finished: bool,
    /// If true, the outbound side of the muxer has closed earlier.
    outbound_finished: bool,
    /// List of substreams we are currently opening.
    outbound_substreams: SmallVec<[(TUserData, TMuxer::OutboundSubstream); 8]>,
    /// Task to notify when a new element is added to `outbound_substreams`, so that we can start
    /// polling it.
    to_notify: Option<task::Task>,
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
            inbound_finished: false,
            outbound_finished: false,
            outbound_substreams: SmallVec::new(),
            to_notify: None,
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

        if let Some(task) = self.to_notify.take() {
            task.notify();
        }

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

impl<TMuxer, TUserData> Stream for NodeStream<TMuxer, TUserData>
where
    TMuxer: muxing::StreamMuxer,
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

        // Closing the node if there's no way we can do anything more.
        if self.inbound_finished && self.outbound_finished && self.outbound_substreams.is_empty() {
            return Ok(Async::Ready(None));
        }

        // Nothing happened. Register our task to be notified and return.
        self.to_notify = Some(task::current());
        Ok(Async::NotReady)
    }
}

impl<TMuxer, TUserData> fmt::Debug for NodeStream<TMuxer, TUserData>
where
    TMuxer: muxing::StreamMuxer,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct("NodeStream")
            .field("inbound_finished", &self.inbound_finished)
            .field("outbound_finished", &self.outbound_finished)
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
