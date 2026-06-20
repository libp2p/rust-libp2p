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

//! Implementation of the [Yamux](https://github.com/hashicorp/yamux/blob/master/spec.md)  multiplexing protocol for libp2p.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use std::{
    collections::VecDeque,
    io,
    io::{IoSlice, IoSliceMut},
    iter,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use futures::{prelude::*, ready};
use libp2p_core::{
    muxing::{StreamMuxer, StreamMuxerEvent},
    upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade, UpgradeInfo},
};
use thiserror::Error;
use yamux::{Connection, ConnectionError};

/// A Yamux connection.
#[derive(Debug)]
pub struct Muxer<C> {
    connection: Connection<C>,
    /// Temporarily buffers inbound streams in case our node is
    /// performing backpressure on the remote.
    ///
    /// The only way how yamux can make progress is by calling
    /// [`yamux::Connection::poll_next_inbound`]. However, the [`StreamMuxer`] interface is
    /// designed to allow a caller to selectively make progress via
    /// [`StreamMuxer::poll_inbound`] and [`StreamMuxer::poll_outbound`] whilst the more general
    /// [`StreamMuxer::poll`] is designed to make progress on existing streams etc.
    ///
    /// This buffer stores inbound streams that are created whilst [`StreamMuxer::poll`] is called.
    /// Once the buffer is full, new inbound streams are dropped.
    inbound_stream_buffer: VecDeque<Stream>,
    /// Waker to be called when new inbound streams are available.
    inbound_stream_waker: Option<Waker>,
}

/// How many streams to buffer before we start resetting them.
///
/// This is equal to the ACK BACKLOG in `rust-yamux`.
/// Thus, for peers running on a recent version of `rust-libp2p`, we should never need to reset
/// streams because they'll voluntarily stop opening them once they hit the ACK backlog.
const MAX_BUFFERED_INBOUND_STREAMS: usize = 256;

impl<C> Muxer<C>
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    /// Create a new Yamux connection.
    fn new(connection: Connection<C>) -> Self {
        Muxer {
            connection,
            inbound_stream_buffer: VecDeque::default(),
            inbound_stream_waker: None,
        }
    }
}

impl<C> StreamMuxer for Muxer<C>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    type Substream = Stream;
    type Error = Error;

    #[tracing::instrument(level = "trace", name = "StreamMuxer::poll_inbound", skip(self, cx))]
    fn poll_inbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        if let Some(stream) = self.inbound_stream_buffer.pop_front() {
            return Poll::Ready(Ok(stream));
        }

        if let Poll::Ready(res) = self.poll_inner(cx) {
            return Poll::Ready(res);
        }

        self.inbound_stream_waker = Some(cx.waker().clone());
        Poll::Pending
    }

    #[tracing::instrument(level = "trace", name = "StreamMuxer::poll_outbound", skip(self, cx))]
    fn poll_outbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let stream = ready!(self.connection.poll_new_outbound(cx))
            .map(Stream)
            .map_err(Error)?;
        Poll::Ready(Ok(stream))
    }

    #[tracing::instrument(level = "trace", name = "StreamMuxer::poll_close", skip(self, cx))]
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.connection.poll_close(cx).map_err(Error)
    }

    #[tracing::instrument(level = "trace", name = "StreamMuxer::poll", skip(self, cx))]
    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        let this = self.get_mut();

        let inbound_stream = ready!(this.poll_inner(cx))?;

        if this.inbound_stream_buffer.len() >= MAX_BUFFERED_INBOUND_STREAMS {
            tracing::warn!(
                stream=%inbound_stream.0,
                "dropping stream because buffer is full"
            );
            drop(inbound_stream);
        } else {
            this.inbound_stream_buffer.push_back(inbound_stream);

            if let Some(waker) = this.inbound_stream_waker.take() {
                waker.wake()
            }
        }

        // Schedule an immediate wake-up, allowing other code to run.
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// A stream produced by the yamux multiplexer.
#[derive(Debug)]
pub struct Stream(yamux::Stream);

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }

    fn poll_read_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_read_vectored(cx, bufs)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write_vectored(cx, bufs)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_close(cx)
    }
}

impl<C> Muxer<C>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    fn poll_inner(&mut self, cx: &mut Context<'_>) -> Poll<Result<Stream, Error>> {
        let stream = ready!(self.connection.poll_next_inbound(cx))
            .ok_or(Error(yamux::ConnectionError::Closed))?
            .map_err(Error)
            .map(Stream)?;

        Poll::Ready(Ok(stream))
    }
}

/// The yamux configuration.
#[derive(Debug, Clone, Default)]
pub struct Config(yamux::Config);

impl Config {
    /// Sets the maximum number of concurrent substreams.
    pub fn set_max_num_streams(&mut self, num_streams: usize) -> &mut Self {
        self.0.set_max_num_streams(num_streams);
        self
    }
}

impl UpgradeInfo for Config {
    type Info = &'static str;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once("/yamux/1.0.0")
    }
}

impl<C> InboundConnectionUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = Muxer<C>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, io: C, _: Self::Info) -> Self::Future {
        future::ready(Ok(Muxer::new(Connection::new(
            io,
            self.0,
            yamux::Mode::Server,
        ))))
    }
}

impl<C> OutboundConnectionUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = Muxer<C>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, io: C, _: Self::Info) -> Self::Future {
        future::ready(Ok(Muxer::new(Connection::new(
            io,
            self.0,
            yamux::Mode::Client,
        ))))
    }
}

/// The Yamux [`StreamMuxer`] error type.
#[derive(Debug, Error)]
#[error(transparent)]
pub struct Error(ConnectionError);

impl From<Error> for io::Error {
    fn from(err: Error) -> Self {
        io::Error::other(err)
    }
}
