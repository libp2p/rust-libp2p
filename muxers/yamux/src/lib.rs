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

//! Implements the Yamux multiplexing protocol for libp2p, see also the
//! [specification](https://github.com/hashicorp/yamux/blob/master/spec.md).

use futures::{
    future,
    prelude::*,
    stream::{BoxStream, LocalBoxStream},
};
use libp2p_core::muxing::StreamMuxer;
use libp2p_core::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_core::Multiaddr;
use std::{
    fmt, io, iter, mem,
    pin::Pin,
    task::{Context, Poll},
};
use thiserror::Error;
use yamux::ConnectionError;

/// A Yamux connection.
pub struct Yamux<S> {
    /// The [`futures::stream::Stream`] of incoming substreams.
    incoming: S,
    /// Handle to control the connection.
    control: yamux::Control,
}

impl<S> fmt::Debug for Yamux<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Yamux")
    }
}

/// A token to poll for an outbound substream.
#[derive(Debug)]
pub struct OpenSubstreamToken(());

impl<C> Yamux<Incoming<C>>
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    /// Create a new Yamux connection.
    fn new(io: C, cfg: yamux::Config, mode: yamux::Mode) -> Self {
        let conn = yamux::Connection::new(io, cfg, mode);
        let ctrl = conn.control();

        Yamux {
            incoming: Incoming {
                stream: yamux::into_stream(conn).err_into().boxed(),
                _marker: std::marker::PhantomData,
            },
            control: ctrl,
        }
    }
}

impl<C> Yamux<LocalIncoming<C>>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    /// Create a new Yamux connection (which is ![`Send`]).
    fn local(io: C, cfg: yamux::Config, mode: yamux::Mode) -> Self {
        let conn = yamux::Connection::new(io, cfg, mode);
        let ctrl = conn.control();

        Yamux {
            incoming: LocalIncoming {
                stream: yamux::into_stream(conn).err_into().boxed_local(),
                _marker: std::marker::PhantomData,
            },
            control: ctrl,
        }
    }
}

pub type YamuxResult<T> = Result<T, YamuxError>;

impl<S> StreamMuxer for Yamux<S>
where
    S: Stream<Item = Result<yamux::Stream, YamuxError>> + Unpin,
{
    type Substream = yamux::Stream;
    type Error = YamuxError;

    fn poll_inbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        self.incoming.poll_next_unpin(cx).map(|maybe_stream| {
            let stream = maybe_stream
                .transpose()?
                .ok_or(YamuxError(ConnectionError::Closed))?;

            Ok(stream)
        })
    }

    fn poll_outbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        Pin::new(&mut self.control)
            .poll_open_stream(cx)
            .map_err(YamuxError)
    }

    fn poll_address_change(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Result<Multiaddr, Self::Error>> {
        Poll::Pending
    }

    fn poll_close(mut self: Pin<&mut Self>, c: &mut Context<'_>) -> Poll<YamuxResult<()>> {
        if let Poll::Ready(()) = Pin::new(&mut self.control)
            .poll_close(c)
            .map_err(YamuxError)?
        {
            return Poll::Ready(Ok(()));
        }

        while let Poll::Ready(maybe_inbound_stream) = self.incoming.poll_next_unpin(c)? {
            match maybe_inbound_stream {
                Some(inbound_stream) => mem::drop(inbound_stream),
                None => return Poll::Ready(Ok(())),
            }
        }

        Poll::Pending
    }
}

/// The yamux configuration.
#[derive(Debug, Clone)]
pub struct YamuxConfig {
    inner: yamux::Config,
    mode: Option<yamux::Mode>,
}

/// The window update mode determines when window updates are
/// sent to the remote, giving it new credit to send more data.
pub struct WindowUpdateMode(yamux::WindowUpdateMode);

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
    pub fn on_receive() -> Self {
        WindowUpdateMode(yamux::WindowUpdateMode::OnReceive)
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
        WindowUpdateMode(yamux::WindowUpdateMode::OnRead)
    }
}

/// The yamux configuration for upgrading I/O resources which are ![`Send`].
#[derive(Clone)]
pub struct YamuxLocalConfig(YamuxConfig);

impl YamuxConfig {
    /// Creates a new `YamuxConfig` in client mode, regardless of whether
    /// it will be used for an inbound or outbound upgrade.
    pub fn client() -> Self {
        Self {
            mode: Some(yamux::Mode::Client),
            ..Default::default()
        }
    }

    /// Creates a new `YamuxConfig` in server mode, regardless of whether
    /// it will be used for an inbound or outbound upgrade.
    pub fn server() -> Self {
        Self {
            mode: Some(yamux::Mode::Server),
            ..Default::default()
        }
    }

    /// Sets the size (in bytes) of the receive window per substream.
    pub fn set_receive_window_size(&mut self, num_bytes: u32) -> &mut Self {
        self.inner.set_receive_window(num_bytes);
        self
    }

    /// Sets the maximum size (in bytes) of the receive buffer per substream.
    pub fn set_max_buffer_size(&mut self, num_bytes: usize) -> &mut Self {
        self.inner.set_max_buffer_size(num_bytes);
        self
    }

    /// Sets the maximum number of concurrent substreams.
    pub fn set_max_num_streams(&mut self, num_streams: usize) -> &mut Self {
        self.inner.set_max_num_streams(num_streams);
        self
    }

    /// Sets the window update mode that determines when the remote
    /// is given new credit for sending more data.
    pub fn set_window_update_mode(&mut self, mode: WindowUpdateMode) -> &mut Self {
        self.inner.set_window_update_mode(mode.0);
        self
    }

    /// Converts the config into a [`YamuxLocalConfig`] for use with upgrades
    /// of I/O streams that are ![`Send`].
    pub fn into_local(self) -> YamuxLocalConfig {
        YamuxLocalConfig(self)
    }
}

impl Default for YamuxConfig {
    fn default() -> Self {
        let mut inner = yamux::Config::default();
        // For conformity with mplex, read-after-close on a multiplexed
        // connection is never permitted and not configurable.
        inner.set_read_after_close(false);
        YamuxConfig { inner, mode: None }
    }
}

impl UpgradeInfo for YamuxConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/yamux/1.0.0")
    }
}

impl UpgradeInfo for YamuxLocalConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/yamux/1.0.0")
    }
}

impl<C> InboundUpgrade<C> for YamuxConfig
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = Yamux<Incoming<C>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, io: C, _: Self::Info) -> Self::Future {
        let mode = self.mode.unwrap_or(yamux::Mode::Server);
        future::ready(Ok(Yamux::new(io, self.inner, mode)))
    }
}

impl<C> InboundUpgrade<C> for YamuxLocalConfig
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    type Output = Yamux<LocalIncoming<C>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, io: C, _: Self::Info) -> Self::Future {
        let cfg = self.0;
        let mode = cfg.mode.unwrap_or(yamux::Mode::Server);
        future::ready(Ok(Yamux::local(io, cfg.inner, mode)))
    }
}

impl<C> OutboundUpgrade<C> for YamuxConfig
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = Yamux<Incoming<C>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, io: C, _: Self::Info) -> Self::Future {
        let mode = self.mode.unwrap_or(yamux::Mode::Client);
        future::ready(Ok(Yamux::new(io, self.inner, mode)))
    }
}

impl<C> OutboundUpgrade<C> for YamuxLocalConfig
where
    C: AsyncRead + AsyncWrite + Unpin + 'static,
{
    type Output = Yamux<LocalIncoming<C>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, io: C, _: Self::Info) -> Self::Future {
        let cfg = self.0;
        let mode = cfg.mode.unwrap_or(yamux::Mode::Client);
        future::ready(Ok(Yamux::local(io, cfg.inner, mode)))
    }
}

/// The Yamux [`StreamMuxer`] error type.
#[derive(Debug, Error)]
#[error("yamux error: {0}")]
pub struct YamuxError(#[from] yamux::ConnectionError);

impl From<YamuxError> for io::Error {
    fn from(err: YamuxError) -> Self {
        match err.0 {
            yamux::ConnectionError::Io(e) => e,
            e => io::Error::new(io::ErrorKind::Other, e),
        }
    }
}

/// The [`futures::stream::Stream`] of incoming substreams.
pub struct Incoming<T> {
    stream: BoxStream<'static, Result<yamux::Stream, YamuxError>>,
    _marker: std::marker::PhantomData<T>,
}

impl<T> fmt::Debug for Incoming<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Incoming")
    }
}

/// The [`futures::stream::Stream`] of incoming substreams (`!Send`).
pub struct LocalIncoming<T> {
    stream: LocalBoxStream<'static, Result<yamux::Stream, YamuxError>>,
    _marker: std::marker::PhantomData<T>,
}

impl<T> fmt::Debug for LocalIncoming<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LocalIncoming")
    }
}

impl<T> Stream for Incoming<T> {
    type Item = Result<yamux::Stream, YamuxError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.stream.as_mut().poll_next_unpin(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

impl<T> Unpin for Incoming<T> {}

impl<T> Stream for LocalIncoming<T> {
    type Item = Result<yamux::Stream, YamuxError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.stream.as_mut().poll_next_unpin(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

impl<T> Unpin for LocalIncoming<T> {}
