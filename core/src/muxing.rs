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
use std::io::Error as IoError;
use std::ops::Deref;

/// Implemented on objects that can open and manage substreams.
pub trait StreamMuxer {
    /// Type of the object that represents the raw substream where data can be read and written.
    type Substream;

    /// Future that will be resolved when the outgoing substream is open.
    type OutboundSubstream;

    /// Polls for an inbound substream.
    ///
    /// This function behaves the same as a `Stream`.
    fn poll_inbound(&self) -> Poll<Option<Self::Substream>, IoError>;

    /// Opens a new outgoing substream, and produces a future that will be resolved when it becomes
    /// available.
    fn open_outbound(&self) -> Self::OutboundSubstream;

    /// Polls the outbound substream.
    ///
    /// May panic or produce an undefined result if an earlier polling returned `Ready` or `Err`.
    fn poll_outbound(&self, substream: &mut Self::OutboundSubstream) -> Poll<Option<Self::Substream>, IoError>;

    /// Destroys an outbound substream. Use this after the outbound substream has finished, or if
    /// you want to interrupt it.
    fn destroy_outbound(&self, substream: Self::OutboundSubstream);

    /// Reads data from a substream. The behaviour is the same as `std::io::Read::read`.
    fn read_substream(&self, substream: &mut Self::Substream, buf: &mut [u8]) -> Result<usize, IoError>;

    /// Write data to a substream. The behaviour is the same as `std::io::Write::write`.
    fn write_substream(&self, substream: &mut Self::Substream, buf: &[u8]) -> Result<usize, IoError>;

    /// Flushes a substream. The behaviour is the same as `std::io::Write::flush`.
    fn flush_substream(&self, substream: &mut Self::Substream) -> Result<(), IoError>;

    /// Attempts to shut down a substream. The behaviour is the same as
    /// `tokio_io::AsyncWrite::shutdown`.
    fn shutdown_substream(&self, substream: &mut Self::Substream) -> Poll<(), IoError>;

    /// Destroys a substream.
    fn destroy_substream(&self, substream: Self::Substream);
}

/// Builds a new future for an outbound substream, where the muxer is a reference.
pub fn outbound_from_ref<P>(muxer: P) -> OutboundSubstreamArcFuture<P>
    where P: Deref, P::Target: StreamMuxer
{
    let outbound = muxer.open_outbound();
    OutboundSubstreamArcFuture {
        muxer,
        outbound: Some(outbound),
    }
}

/// Future returned by `outbound_from_ref`.
pub struct OutboundSubstreamArcFuture<P>
    where P: Deref, P::Target: StreamMuxer
{
    muxer: P,
    outbound: Option<<P::Target as StreamMuxer>::OutboundSubstream>,
}

impl<P> Future for OutboundSubstreamArcFuture<P>
    where P: Deref, P::Target: StreamMuxer
{
    type Item = Option<<P::Target as StreamMuxer>::Substream>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.muxer.poll_outbound(self.outbound.as_mut().expect("outbound was empty"))
    }
}

impl<P> Drop for OutboundSubstreamArcFuture<P>
    where P: Deref, P::Target: StreamMuxer
{
    fn drop(&mut self) {
        self.muxer.destroy_outbound(self.outbound.take().expect("outbound was empty"))
    }
}
