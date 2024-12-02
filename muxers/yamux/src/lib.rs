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

use either::Either;
use futures::{prelude::*, ready};
use libp2p_core::{
    muxing::{StreamMuxer, StreamMuxerEvent},
    upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade, UpgradeInfo},
};
use thiserror::Error;

/// A Yamux connection.
#[derive(Debug)]
pub struct Muxer<C> {
    connection: Either<yamux012::Connection<C>, yamux013::Connection<C>>,
    /// Temporarily buffers inbound streams in case our node is
    /// performing backpressure on the remote.
    ///
    /// The only way how yamux can make progress is by calling
    /// [`yamux013::Connection::poll_next_inbound`]. However, the [`StreamMuxer`] interface is
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
    fn new(connection: Either<yamux012::Connection<C>, yamux013::Connection<C>>) -> Self {
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
        let stream = match self.connection.as_mut() {
            Either::Left(c) => ready!(c.poll_new_outbound(cx))
                .map_err(|e| Error(Either::Left(e)))
                .map(|s| Stream(Either::Left(s))),
            Either::Right(c) => ready!(c.poll_new_outbound(cx))
                .map_err(|e| Error(Either::Right(e)))
                .map(|s| Stream(Either::Right(s))),
        }?;
        Poll::Ready(Ok(stream))
    }

    #[tracing::instrument(level = "trace", name = "StreamMuxer::poll_close", skip(self, cx))]
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.connection.as_mut() {
            Either::Left(c) => c.poll_close(cx).map_err(|e| Error(Either::Left(e))),
            Either::Right(c) => c.poll_close(cx).map_err(|e| Error(Either::Right(e))),
        }
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
pub struct Stream(Either<yamux012::Stream, yamux013::Stream>);

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        either::for_both!(self.0.as_mut(), s => Pin::new(s).poll_read(cx, buf))
    }

    fn poll_read_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>> {
        either::for_both!(self.0.as_mut(), s => Pin::new(s).poll_read_vectored(cx, bufs))
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        either::for_both!(self.0.as_mut(), s => Pin::new(s).poll_write(cx, buf))
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        either::for_both!(self.0.as_mut(), s => Pin::new(s).poll_write_vectored(cx, bufs))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        either::for_both!(self.0.as_mut(), s => Pin::new(s).poll_flush(cx))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        either::for_both!(self.0.as_mut(), s => Pin::new(s).poll_close(cx))
    }
}

impl<C> Muxer<C>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    fn poll_inner(&mut self, cx: &mut Context<'_>) -> Poll<Result<Stream, Error>> {
        let stream = match self.connection.as_mut() {
            Either::Left(c) => ready!(c.poll_next_inbound(cx))
                .ok_or(Error(Either::Left(yamux012::ConnectionError::Closed)))?
                .map_err(|e| Error(Either::Left(e)))
                .map(|s| Stream(Either::Left(s)))?,
            Either::Right(c) => ready!(c.poll_next_inbound(cx))
                .ok_or(Error(Either::Right(yamux013::ConnectionError::Closed)))?
                .map_err(|e| Error(Either::Right(e)))
                .map(|s| Stream(Either::Right(s)))?,
        };

        Poll::Ready(Ok(stream))
    }
}

/// The yamux configuration.
#[derive(Debug, Clone)]
pub struct Config(Either<Config012, Config013>);

impl Default for Config {
    fn default() -> Self {
        Self(Either::Right(Config013::default()))
    }
}

#[derive(Debug, Clone)]
struct Config012 {
    inner: yamux012::Config,
    mode: Option<yamux012::Mode>,
}

impl Default for Config012 {
    fn default() -> Self {
        let mut inner = yamux012::Config::default();
        // For conformity with mplex, read-after-close on a multiplexed
        // connection is never permitted and not configurable.
        inner.set_read_after_close(false);
        Self { inner, mode: None }
    }
}

/// The window update mode determines when window updates are
/// sent to the remote, giving it new credit to send more data.
pub struct WindowUpdateMode(yamux012::WindowUpdateMode);

impl WindowUpdateMode {
    /// The window update mode whereby the remote is given
    /// new credit via a window update whenever the current
    /// receive window is exhausted when data is received,
    /// i.e. this mode cannot exert back-pressure from application
    /// code that is slow to read from a substream.
    ///
    /// > **Note**: The receive buffer may overflow with this
    /// > strategy if the receiver is too slow in reading the
    /// > data from the buffer. The maximum receive buffer
    /// > size must be tuned appropriately for the desired
    /// > throughput and level of tolerance for (temporarily)
    /// > slow receivers.
    #[deprecated(note = "Use `WindowUpdateMode::on_read` instead.")]
    pub fn on_receive() -> Self {
        #[allow(deprecated)]
        WindowUpdateMode(yamux012::WindowUpdateMode::OnReceive)
    }

    /// The window update mode whereby the remote is given new
    /// credit only when the current receive window is exhausted
    /// when data is read from the substream's receive buffer,
    /// i.e. application code that is slow to read from a substream
    /// exerts back-pressure on the remote.
    ///
    /// > **Note**: If the receive window of a substream on
    /// > both peers is exhausted and both peers are blocked on
    /// > sending data before reading from the stream, a deadlock
    /// > occurs. To avoid this situation, reading from a substream
    /// > should never be blocked on writing to the same substream.
    ///
    /// > **Note**: With this strategy, there is usually no point in the
    /// > receive buffer being larger than the window size.
    pub fn on_read() -> Self {
        WindowUpdateMode(yamux012::WindowUpdateMode::OnRead)
    }
}

impl Config {
    /// Creates a new `YamuxConfig` in client mode, regardless of whether
    /// it will be used for an inbound or outbound upgrade.
    #[deprecated(note = "Will be removed with the next breaking release.")]
    pub fn client() -> Self {
        Self(Either::Left(Config012 {
            mode: Some(yamux012::Mode::Client),
            ..Default::default()
        }))
    }

    /// Creates a new `YamuxConfig` in server mode, regardless of whether
    /// it will be used for an inbound or outbound upgrade.
    #[deprecated(note = "Will be removed with the next breaking release.")]
    pub fn server() -> Self {
        Self(Either::Left(Config012 {
            mode: Some(yamux012::Mode::Server),
            ..Default::default()
        }))
    }

    /// Sets the size (in bytes) of the receive window per substream.
    #[deprecated(
        note = "Will be replaced in the next breaking release with a connection receive window size limit."
    )]
    pub fn set_receive_window_size(&mut self, num_bytes: u32) -> &mut Self {
        self.set(|cfg| cfg.set_receive_window(num_bytes))
    }

    /// Sets the maximum size (in bytes) of the receive buffer per substream.
    #[deprecated(note = "Will be removed with the next breaking release.")]
    pub fn set_max_buffer_size(&mut self, num_bytes: usize) -> &mut Self {
        self.set(|cfg| cfg.set_max_buffer_size(num_bytes))
    }

    /// Sets the maximum number of concurrent substreams.
    pub fn set_max_num_streams(&mut self, num_streams: usize) -> &mut Self {
        self.set(|cfg| cfg.set_max_num_streams(num_streams))
    }

    /// Sets the window update mode that determines when the remote
    /// is given new credit for sending more data.
    #[deprecated(
        note = "`WindowUpdate::OnRead` is the default. `WindowUpdate::OnReceive` breaks backpressure, is thus not recommended, and will be removed in the next breaking release. Thus this method becomes obsolete and will be removed with the next breaking release."
    )]
    pub fn set_window_update_mode(&mut self, mode: WindowUpdateMode) -> &mut Self {
        self.set(|cfg| cfg.set_window_update_mode(mode.0))
    }

    fn set(&mut self, f: impl FnOnce(&mut yamux012::Config) -> &mut yamux012::Config) -> &mut Self {
        let cfg012 = match self.0.as_mut() {
            Either::Left(c) => &mut c.inner,
            Either::Right(_) => {
                self.0 = Either::Left(Config012::default());
                &mut self.0.as_mut().unwrap_left().inner
            }
        };

        f(cfg012);

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
        let connection = match self.0 {
            Either::Left(Config012 { inner, mode }) => Either::Left(yamux012::Connection::new(
                io,
                inner,
                mode.unwrap_or(yamux012::Mode::Server),
            )),
            Either::Right(Config013(cfg)) => {
                Either::Right(yamux013::Connection::new(io, cfg, yamux013::Mode::Server))
            }
        };

        future::ready(Ok(Muxer::new(connection)))
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
        let connection = match self.0 {
            Either::Left(Config012 { inner, mode }) => Either::Left(yamux012::Connection::new(
                io,
                inner,
                mode.unwrap_or(yamux012::Mode::Client),
            )),
            Either::Right(Config013(cfg)) => {
                Either::Right(yamux013::Connection::new(io, cfg, yamux013::Mode::Client))
            }
        };

        future::ready(Ok(Muxer::new(connection)))
    }
}

#[derive(Debug, Clone)]
struct Config013(yamux013::Config);

impl Default for Config013 {
    fn default() -> Self {
        let mut cfg = yamux013::Config::default();
        // For conformity with mplex, read-after-close on a multiplexed
        // connection is never permitted and not configurable.
        cfg.set_read_after_close(false);
        Self(cfg)
    }
}

/// The Yamux [`StreamMuxer`] error type.
#[derive(Debug, Error)]
#[error(transparent)]
pub struct Error(Either<yamux012::ConnectionError, yamux013::ConnectionError>);

impl From<Error> for io::Error {
    fn from(err: Error) -> Self {
        match err.0 {
            Either::Left(err) => match err {
                yamux012::ConnectionError::Io(e) => e,
                e => io::Error::new(io::ErrorKind::Other, e),
            },
            Either::Right(err) => match err {
                yamux013::ConnectionError::Io(e) => e,
                e => io::Error::new(io::ErrorKind::Other, e),
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn config_set_switches_to_v012() {
        // By default we use yamux v0.13. Thus we provide the benefits of yamux v0.13 to all users
        // that do not depend on any of the behaviors (i.e. configuration options) of v0.12.
        let mut cfg = Config::default();
        assert!(matches!(
            cfg,
            Config(Either::Right(Config013(yamux013::Config { .. })))
        ));

        // In case a user makes any configurations, use yamux v0.12 instead.
        cfg.set_max_num_streams(42);
        assert!(matches!(cfg, Config(Either::Left(Config012 { .. }))));
    }
}
