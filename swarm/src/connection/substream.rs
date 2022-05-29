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
use libp2p_core::multiaddr::Multiaddr;
use libp2p_core::muxing::{substream_from_ref, StreamMuxer, StreamMuxerEvent, SubstreamRef};
use smallvec::SmallVec;
use std::sync::Arc;
use std::{fmt, io::Error as IoError, pin::Pin, task::Context, task::Poll};

/// Endpoint for a received substream.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum SubstreamEndpoint<TDialInfo> {
    Dialer(TDialInfo),
    Listener,
}

/// Implementation of `Stream` that handles substream multiplexing.
///
/// The stream will receive substreams and can be used to open new outgoing substreams. Destroying
/// the `Muxing` will **not** close the existing substreams.
///
/// The stream will close once both the inbound and outbound channels are closed, and no more
/// outbound substream attempt is pending.
pub struct Muxing<TMuxer, TUserData>
where
    TMuxer: StreamMuxer,
{
    /// The muxer used to manage substreams.
    inner: Arc<TMuxer>,
    /// List of substreams we are currently opening.
    outbound_substreams: SmallVec<[(TUserData, TMuxer::OutboundSubstream); 8]>,
}

/// Future that signals the remote that we have closed the connection.
pub struct Close<TMuxer> {
    /// Muxer to close.
    muxer: Arc<TMuxer>,
}

/// A successfully opened substream.
pub type Substream<TMuxer> = SubstreamRef<Arc<TMuxer>>;

/// Event that can happen on the `Muxing`.
pub enum SubstreamEvent<TMuxer, TUserData>
where
    TMuxer: StreamMuxer,
{
    /// A new inbound substream arrived.
    InboundSubstream {
        /// The newly-opened substream. Will return EOF of an error if the `Muxing` is
        /// destroyed or `close_graceful` is called.
        substream: Substream<TMuxer>,
    },

    /// An outbound substream has successfully been opened.
    OutboundSubstream {
        /// User data that has been passed to the `open_substream` method.
        user_data: TUserData,
        /// The newly-opened substream. Will return EOF of an error if the `Muxing` is
        /// destroyed or `close_graceful` is called.
        substream: Substream<TMuxer>,
    },

    /// Address to the remote has changed. The previous one is now obsolete.
    ///
    /// > **Note**: This can for example happen when using the QUIC protocol, where the two nodes
    /// >           can change their IP address while retaining the same QUIC connection.
    AddressChange(Multiaddr),
}

/// Identifier for a substream being opened.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutboundSubstreamId(usize);

impl<TMuxer, TUserData> Muxing<TMuxer, TUserData>
where
    TMuxer: StreamMuxer,
{
    /// Creates a new node events stream.
    pub fn new(muxer: TMuxer) -> Self {
        Muxing {
            inner: Arc::new(muxer),
            outbound_substreams: SmallVec::new(),
        }
    }

    /// Starts the process of opening a new outbound substream.
    ///
    /// After calling this method, polling the stream should eventually produce either an
    /// `OutboundSubstream` event or an `OutboundClosed` event containing the user data that has
    /// been passed to this method.
    pub fn open_substream(&mut self, user_data: TUserData) {
        let raw = self.inner.open_outbound();
        self.outbound_substreams.push((user_data, raw));
    }

    /// Destroys the node stream and returns all the pending outbound substreams, plus an object
    /// that signals the remote that we shut down the connection.
    #[must_use]
    pub fn close(mut self) -> (Close<TMuxer>, Vec<TUserData>) {
        let substreams = self.cancel_outgoing();
        let close = Close {
            muxer: self.inner.clone(),
        };
        (close, substreams)
    }

    /// Destroys all outbound streams and returns the corresponding user data.
    pub fn cancel_outgoing(&mut self) -> Vec<TUserData> {
        let mut out = Vec::with_capacity(self.outbound_substreams.len());
        for (user_data, outbound) in self.outbound_substreams.drain(..) {
            out.push(user_data);
            self.inner.destroy_outbound(outbound);
        }
        out
    }

    /// Provides an API similar to `Future`.
    pub fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<SubstreamEvent<TMuxer, TUserData>, IoError>> {
        // Polling inbound substream.
        match self.inner.poll_event(cx) {
            Poll::Ready(Ok(StreamMuxerEvent::InboundSubstream(substream))) => {
                let substream = substream_from_ref(self.inner.clone(), substream);
                return Poll::Ready(Ok(SubstreamEvent::InboundSubstream { substream }));
            }
            Poll::Ready(Ok(StreamMuxerEvent::AddressChange(addr))) => {
                return Poll::Ready(Ok(SubstreamEvent::AddressChange(addr)))
            }
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err.into())),
            Poll::Pending => {}
        }

        // Polling outbound substreams.
        // We remove each element from `outbound_substreams` one by one and add them back.
        for n in (0..self.outbound_substreams.len()).rev() {
            let (user_data, mut outbound) = self.outbound_substreams.swap_remove(n);
            match self.inner.poll_outbound(cx, &mut outbound) {
                Poll::Ready(Ok(substream)) => {
                    let substream = substream_from_ref(self.inner.clone(), substream);
                    self.inner.destroy_outbound(outbound);
                    return Poll::Ready(Ok(SubstreamEvent::OutboundSubstream {
                        user_data,
                        substream,
                    }));
                }
                Poll::Pending => {
                    self.outbound_substreams.push((user_data, outbound));
                }
                Poll::Ready(Err(err)) => {
                    self.inner.destroy_outbound(outbound);
                    return Poll::Ready(Err(err.into()));
                }
            }
        }

        // Nothing happened. Register our task to be notified and return.
        Poll::Pending
    }
}

impl<TMuxer, TUserData> fmt::Debug for Muxing<TMuxer, TUserData>
where
    TMuxer: StreamMuxer,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("Muxing")
            .field("outbound_substreams", &self.outbound_substreams.len())
            .finish()
    }
}

impl<TMuxer, TUserData> Drop for Muxing<TMuxer, TUserData>
where
    TMuxer: StreamMuxer,
{
    fn drop(&mut self) {
        // The substreams that were produced will continue to work, as the muxer is held in an Arc.
        // However we will no longer process any further inbound or outbound substream, and we
        // therefore close everything.
        for (_, outbound) in self.outbound_substreams.drain(..) {
            self.inner.destroy_outbound(outbound);
        }
    }
}

impl<TMuxer> Future for Close<TMuxer>
where
    TMuxer: StreamMuxer,
{
    type Output = Result<(), IoError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.muxer.poll_close(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err.into())),
        }
    }
}

impl<TMuxer> fmt::Debug for Close<TMuxer>
where
    TMuxer: StreamMuxer,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("Close").finish()
    }
}

impl<TMuxer, TUserData> fmt::Debug for SubstreamEvent<TMuxer, TUserData>
where
    TMuxer: StreamMuxer,
    TMuxer::Substream: fmt::Debug,
    TUserData: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubstreamEvent::InboundSubstream { substream } => f
                .debug_struct("SubstreamEvent::OutboundClosed")
                .field("substream", substream)
                .finish(),
            SubstreamEvent::OutboundSubstream {
                user_data,
                substream,
            } => f
                .debug_struct("SubstreamEvent::OutboundSubstream")
                .field("user_data", user_data)
                .field("substream", substream)
                .finish(),
            SubstreamEvent::AddressChange(address) => f
                .debug_struct("SubstreamEvent::AddressChange")
                .field("address", address)
                .finish(),
        }
    }
}
