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

mod codec;
mod config;
mod io;

pub use config::{MaxBufferBehaviour, MplexConfig};

use bytes::Bytes;
use codec::LocalStreamId;
use futures::{future, prelude::*, ready};
use libp2p_core::{
    muxing::StreamMuxerEvent,
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo},
    StreamMuxer,
};
use parking_lot::Mutex;
use std::{cmp, iter, pin::Pin, sync::Arc, task::Context, task::Poll};

impl UpgradeInfo for MplexConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(self.protocol_name)
    }
}

impl<C> InboundUpgrade<C> for MplexConfig
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    type Output = Multiplex<C>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, io::Error>>;

    fn upgrade_inbound(self, socket: C, _: Self::Info) -> Self::Future {
        future::ready(Ok(Multiplex {
            io: Arc::new(Mutex::new(io::Multiplexed::new(socket, self))),
        }))
    }
}

impl<C> OutboundUpgrade<C> for MplexConfig
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    type Output = Multiplex<C>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, io::Error>>;

    fn upgrade_outbound(self, socket: C, _: Self::Info) -> Self::Future {
        future::ready(Ok(Multiplex {
            io: Arc::new(Mutex::new(io::Multiplexed::new(socket, self))),
        }))
    }
}

/// Multiplexer. Implements the `StreamMuxer` trait.
///
/// This implementation isn't capable of detecting when the underlying socket changes its address,
/// and no [`StreamMuxerEvent::AddressChange`] event is ever emitted.
pub struct Multiplex<C> {
    io: Arc<Mutex<io::Multiplexed<C>>>,
}

impl<C> StreamMuxer for Multiplex<C>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    type Substream = Substream<C>;
    type OutboundSubstream = OutboundSubstream;
    type Error = io::Error;

    fn poll_event(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<StreamMuxerEvent<Self::Substream>>> {
        let stream_id = ready!(self.io.lock().poll_next_stream(cx))?;
        let stream = Substream::new(stream_id, self.io.clone());
        Poll::Ready(Ok(StreamMuxerEvent::InboundSubstream(stream)))
    }

    fn open_outbound(&self) -> Self::OutboundSubstream {
        OutboundSubstream {}
    }

    fn poll_outbound(
        &self,
        cx: &mut Context<'_>,
        _: &mut Self::OutboundSubstream,
    ) -> Poll<Result<Self::Substream, io::Error>> {
        let stream_id = ready!(self.io.lock().poll_open_stream(cx))?;
        Poll::Ready(Ok(Substream::new(stream_id, self.io.clone())))
    }

    fn destroy_outbound(&self, _substream: Self::OutboundSubstream) {
        // Nothing to do, since `open_outbound` creates no new local state.
    }

    fn read_substream(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::Substream,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(substream).poll_read(cx, buf)
    }

    fn write_substream(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::Substream,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(substream).poll_write(cx, buf)
    }

    fn flush_substream(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::Substream,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(substream).poll_flush(cx)
    }

    fn shutdown_substream(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::Substream,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(substream).poll_close(cx)
    }

    fn destroy_substream(&self, sub: Self::Substream) {
        std::mem::drop(sub)
    }

    fn poll_close(&self, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        self.io.lock().poll_close(cx)
    }
}

/// Active attempt to open an outbound substream.
pub struct OutboundSubstream {}

impl<C> AsyncRead for Substream<C>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        loop {
            // Try to read from the current (i.e. last received) frame.
            if !this.current_data.is_empty() {
                let len = cmp::min(this.current_data.len(), buf.len());
                buf[..len].copy_from_slice(&this.current_data.split_to(len));
                return Poll::Ready(Ok(len));
            }

            // Read the next data frame from the multiplexed stream.
            match ready!(this.io.lock().poll_read_stream(cx, this.id))? {
                Some(data) => {
                    this.current_data = data;
                }
                None => return Poll::Ready(Ok(0)),
            }
        }
    }
}

impl<C> AsyncWrite for Substream<C>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        this.io.lock().poll_write_stream(cx, this.id, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();

        this.io.lock().poll_flush_stream(cx, this.id)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let mut io = this.io.lock();

        ready!(io.poll_close_stream(cx, this.id))?;
        ready!(io.poll_flush_stream(cx, this.id))?;

        Poll::Ready(Ok(()))
    }
}

/// Active substream to the remote.
pub struct Substream<C>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    /// The unique, local identifier of the substream.
    id: LocalStreamId,
    /// The current data frame the substream is reading from.
    current_data: Bytes,
    /// Shared reference to the actual muxer.
    io: Arc<Mutex<io::Multiplexed<C>>>,
}

impl<C> Substream<C>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    fn new(id: LocalStreamId, io: Arc<Mutex<io::Multiplexed<C>>>) -> Self {
        Self {
            id,
            current_data: Bytes::new(),
            io,
        }
    }
}

impl<C> Drop for Substream<C>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    fn drop(&mut self) {
        self.io.lock().drop_stream(self.id);
    }
}
