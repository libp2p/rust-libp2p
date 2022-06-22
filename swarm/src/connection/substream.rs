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
use libp2p_core::muxing::{OpenFlags, StreamMuxer, StreamMuxerEvent};
use std::collections::VecDeque;
use std::sync::Arc;
use std::{fmt, pin::Pin, task::Context, task::Poll};

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
    outbound_substreams: VecDeque<TUserData>,
}

/// Future that signals the remote that we have closed the connection.
pub struct Close<TMuxer> {
    /// Muxer to close.
    muxer: Arc<TMuxer>,
}

/// Event that can happen on the `Muxing`.
pub enum SubstreamEvent<TMuxer, TUserData>
where
    TMuxer: StreamMuxer,
{
    /// A new inbound substream arrived.
    InboundSubstream {
        /// The newly-opened substream. Will return EOF of an error if the `Muxing` is
        /// destroyed or `close_graceful` is called.
        substream: TMuxer::Substream,
    },

    /// An outbound substream has successfully been opened.
    OutboundSubstream {
        /// User data that has been passed to the `open_substream` method.
        user_data: TUserData,
        /// The newly-opened substream. Will return EOF of an error if the `Muxing` is
        /// destroyed or `close_graceful` is called.
        substream: TMuxer::Substream,
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
            outbound_substreams: VecDeque::with_capacity(8),
        }
    }

    /// Starts the process of opening a new outbound substream.
    ///
    /// After calling this method, polling the stream should eventually produce either an
    /// `OutboundSubstream` event or an `OutboundClosed` event containing the user data that has
    /// been passed to this method.
    pub fn open_substream(&mut self, user_data: TUserData) {
        self.outbound_substreams.push_back(user_data);
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
        for user_data in self.outbound_substreams.drain(..) {
            out.push(user_data);
        }
        out
    }

    /// Provides an API similar to `Future`.
    pub fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<SubstreamEvent<TMuxer, TUserData>, TMuxer::Error>> {
        let mut flags = OpenFlags::default();
        if !self.outbound_substreams.is_empty() {
            flags.insert(OpenFlags::OUTBOUND);
        }

        match self.inner.poll_event(flags, cx) {
            Poll::Ready(Ok(StreamMuxerEvent::InboundSubstream(substream))) => {
                return Poll::Ready(Ok(SubstreamEvent::InboundSubstream { substream }));
            }
            Poll::Ready(Ok(StreamMuxerEvent::OutboundSubstream(substream))) => {
                return Poll::Ready(Ok(SubstreamEvent::OutboundSubstream {
                    substream,
                    user_data: self
                        .outbound_substreams
                        .pop_front()
                        .expect("we checked that we are not empty"),
                }));
            }
            Poll::Ready(Ok(StreamMuxerEvent::AddressChange(addr))) => {
                return Poll::Ready(Ok(SubstreamEvent::AddressChange(addr)))
            }
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Pending => {}
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

impl<TMuxer> Future for Close<TMuxer>
where
    TMuxer: StreamMuxer,
{
    type Output = Result<(), TMuxer::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.muxer.poll_close(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
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
