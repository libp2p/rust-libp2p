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

use futures::{future, prelude::*, ready, stream::{BoxStream, LocalBoxStream}};
use libp2p_core::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo, Negotiated};
use parking_lot::Mutex;
use std::{fmt, io, iter, pin::Pin, task::Context};
use thiserror::Error;

/// A Yamux connection.
pub struct Yamux<S>(Mutex<Inner<S>>);

impl<S> fmt::Debug for Yamux<S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Yamux")
    }
}

struct Inner<S> {
    /// The `futures::stream::Stream` of incoming substreams.
    incoming: S,
    /// Handle to control the connection.
    control: yamux::Control,
    /// True, once we have received an inbound substream.
    acknowledged: bool
}

/// A token to poll for an outbound substream.
#[derive(Debug)]
pub struct OpenSubstreamToken(());

impl<C> Yamux<Incoming<C>>
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    /// Create a new Yamux connection.
    pub fn new(io: C, mut cfg: yamux::Config, mode: yamux::Mode) -> Self {
        cfg.set_read_after_close(false);
        let conn = yamux::Connection::new(io, cfg, mode);
        let ctrl = conn.control();
        let inner = Inner {
            incoming: Incoming {
                stream: yamux::into_stream(conn).err_into().boxed(),
                _marker: std::marker::PhantomData
            },
            control: ctrl,
            acknowledged: false
        };
        Yamux(Mutex::new(inner))
    }
}

impl<C> Yamux<LocalIncoming<C>>
where
    C: AsyncRead + AsyncWrite + Unpin + 'static
{
    /// Create a new Yamux connection (which is ![`Send`]).
    pub fn local(io: C, mut cfg: yamux::Config, mode: yamux::Mode) -> Self {
        cfg.set_read_after_close(false);
        let conn = yamux::Connection::new(io, cfg, mode);
        let ctrl = conn.control();
        let inner = Inner {
            incoming: LocalIncoming {
                stream: yamux::into_stream(conn).err_into().boxed_local(),
                _marker: std::marker::PhantomData
            },
            control: ctrl,
            acknowledged: false
        };
        Yamux(Mutex::new(inner))
    }
}

type Poll<T> = std::task::Poll<Result<T, YamuxError>>;

impl<S> libp2p_core::StreamMuxer for Yamux<S>
where
    S: Stream<Item = Result<yamux::Stream, YamuxError>> + Unpin
{
    type Substream = yamux::Stream;
    type OutboundSubstream = OpenSubstreamToken;
    type Error = YamuxError;

    fn poll_inbound(&self, c: &mut Context) -> Poll<Self::Substream> {
        let mut inner = self.0.lock();
        match ready!(inner.incoming.poll_next_unpin(c)) {
            Some(Ok(s)) => {
                inner.acknowledged = true;
                Poll::Ready(Ok(s))
            }
            Some(Err(e)) => Poll::Ready(Err(e)),
            None => Poll::Ready(Err(yamux::ConnectionError::Closed.into()))
        }
    }

    fn open_outbound(&self) -> Self::OutboundSubstream {
        OpenSubstreamToken(())
    }

    fn poll_outbound(&self, c: &mut Context, _: &mut OpenSubstreamToken) -> Poll<Self::Substream> {
        let mut inner = self.0.lock();
        Pin::new(&mut inner.control).poll_open_stream(c).map_err(YamuxError)
    }

    fn destroy_outbound(&self, _: Self::OutboundSubstream) {
        self.0.lock().control.abort_open_stream()
    }

    fn read_substream(&self, c: &mut Context, s: &mut Self::Substream, b: &mut [u8]) -> Poll<usize> {
        Pin::new(s).poll_read(c, b).map_err(|e| YamuxError(e.into()))
    }

    fn write_substream(&self, c: &mut Context, s: &mut Self::Substream, b: &[u8]) -> Poll<usize> {
        Pin::new(s).poll_write(c, b).map_err(|e| YamuxError(e.into()))
    }

    fn flush_substream(&self, c: &mut Context, s: &mut Self::Substream) -> Poll<()> {
        Pin::new(s).poll_flush(c).map_err(|e| YamuxError(e.into()))
    }

    fn shutdown_substream(&self, c: &mut Context, s: &mut Self::Substream) -> Poll<()> {
        Pin::new(s).poll_close(c).map_err(|e| YamuxError(e.into()))
    }

    fn destroy_substream(&self, _: Self::Substream) { }

    fn is_remote_acknowledged(&self) -> bool {
        self.0.lock().acknowledged
    }

    fn close(&self, c: &mut Context) -> Poll<()> {
        let mut inner = self.0.lock();
        Pin::new(&mut inner.control).poll_close(c).map_err(YamuxError)
    }

    fn flush_all(&self, _: &mut Context) -> Poll<()> {
        Poll::Ready(Ok(()))
    }
}

/// The yamux configuration.
#[derive(Clone)]
pub struct Config(yamux::Config);

/// The yamux configuration for upgrading I/O resources which are ![`Send`].
#[derive(Clone)]
pub struct LocalConfig(Config);

impl Config {
    pub fn new(cfg: yamux::Config) -> Self {
        Config(cfg)
    }

    /// Turn this into a `LocalConfig` for use with upgrades of !Send resources.
    pub fn local(self) -> LocalConfig {
        LocalConfig(self)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config(yamux::Config::default())
    }
}

impl UpgradeInfo for Config {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/yamux/1.0.0")
    }
}

impl UpgradeInfo for LocalConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/yamux/1.0.0")
    }
}

impl<C> InboundUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    type Output = Yamux<Incoming<Negotiated<C>>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, io: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ready(Ok(Yamux::new(io, self.0, yamux::Mode::Server)))
    }
}

impl<C> InboundUpgrade<C> for LocalConfig
where
    C: AsyncRead + AsyncWrite + Unpin + 'static
{
    type Output = Yamux<LocalIncoming<Negotiated<C>>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, io: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ready(Ok(Yamux::local(io, (self.0).0, yamux::Mode::Server)))
    }
}

impl<C> OutboundUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    type Output = Yamux<Incoming<Negotiated<C>>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, io: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ready(Ok(Yamux::new(io, self.0, yamux::Mode::Client)))
    }
}

impl<C> OutboundUpgrade<C> for LocalConfig
where
    C: AsyncRead + AsyncWrite + Unpin + 'static
{
    type Output = Yamux<LocalIncoming<Negotiated<C>>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, io: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ready(Ok(Yamux::local(io, (self.0).0, yamux::Mode::Client)))
    }
}

/// The Yamux [`StreamMuxer`] error type.
#[derive(Debug, Error)]
#[error("yamux error: {0}")]
pub struct YamuxError(#[from] pub yamux::ConnectionError);

impl Into<io::Error> for YamuxError {
    fn into(self: YamuxError) -> io::Error {
        io::Error::new(io::ErrorKind::Other, self.to_string())
    }
}

/// The [`futures::stream::Stream`] of incoming substreams.
pub struct Incoming<T> {
    stream: BoxStream<'static, Result<yamux::Stream, YamuxError>>,
    _marker: std::marker::PhantomData<T>
}

impl<T> fmt::Debug for Incoming<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Incoming")
    }
}

/// The [`futures::stream::Stream`] of incoming substreams (`!Send`).
pub struct LocalIncoming<T> {
    stream: LocalBoxStream<'static, Result<yamux::Stream, YamuxError>>,
    _marker: std::marker::PhantomData<T>
}

impl<T> fmt::Debug for LocalIncoming<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("LocalIncoming")
    }
}

impl<T: Unpin> Stream for Incoming<T> {
    type Item = Result<yamux::Stream, YamuxError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> std::task::Poll<Option<Self::Item>> {
        self.stream.as_mut().poll_next_unpin(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

impl<T: Unpin> Stream for LocalIncoming<T> {
    type Item = Result<yamux::Stream, YamuxError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> std::task::Poll<Option<Self::Item>> {
        self.stream.as_mut().poll_next_unpin(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}
