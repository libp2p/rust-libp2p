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
use libp2p_core::muxing::StreamMuxerEvent;
use libp2p_core::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use parking_lot::Mutex;
use std::{fmt, io, iter, ops::{Deref, DerefMut}, pin::Pin, task::Context};
use thiserror::Error;

pub use yamux::{Mode, WindowUpdateMode};

/// A Yamux connection.
///
/// This implementation isn't capable of detecting when the underlying socket changes its address,
/// and no [`StreamMuxerEvent::AddressChange`] event is ever emitted.
pub struct Yamux<S>(Mutex<Inner<S>>);

impl<S> fmt::Debug for Yamux<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Yamux")
    }
}

struct Inner<S> {
    /// The [`futures::stream::Stream`] of incoming substreams.
    incoming: S,
    /// Handle to control the connection.
    control: yamux::Control,
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

    fn poll_event(&self, c: &mut Context<'_>) -> Poll<StreamMuxerEvent<Self::Substream>> {
        let mut inner = self.0.lock();
        match ready!(inner.incoming.poll_next_unpin(c)) {
            Some(Ok(s)) => Poll::Ready(Ok(StreamMuxerEvent::InboundSubstream(s))),
            Some(Err(e)) => Poll::Ready(Err(e)),
            None => Poll::Ready(Err(yamux::ConnectionError::Closed.into()))
        }
    }

    fn open_outbound(&self) -> Self::OutboundSubstream {
        OpenSubstreamToken(())
    }

    fn poll_outbound(&self, c: &mut Context<'_>, _: &mut OpenSubstreamToken) -> Poll<Self::Substream> {
        let mut inner = self.0.lock();
        Pin::new(&mut inner.control).poll_open_stream(c).map_err(YamuxError)
    }

    fn destroy_outbound(&self, _: Self::OutboundSubstream) {
        self.0.lock().control.abort_open_stream()
    }

    fn read_substream(&self, c: &mut Context<'_>, s: &mut Self::Substream, b: &mut [u8]) -> Poll<usize> {
        Pin::new(s).poll_read(c, b).map_err(|e| YamuxError(e.into()))
    }

    fn write_substream(&self, c: &mut Context<'_>, s: &mut Self::Substream, b: &[u8]) -> Poll<usize> {
        Pin::new(s).poll_write(c, b).map_err(|e| YamuxError(e.into()))
    }

    fn flush_substream(&self, c: &mut Context<'_>, s: &mut Self::Substream) -> Poll<()> {
        Pin::new(s).poll_flush(c).map_err(|e| YamuxError(e.into()))
    }

    fn shutdown_substream(&self, c: &mut Context<'_>, s: &mut Self::Substream) -> Poll<()> {
        Pin::new(s).poll_close(c).map_err(|e| YamuxError(e.into()))
    }

    fn destroy_substream(&self, _: Self::Substream) { }

    fn close(&self, c: &mut Context<'_>) -> Poll<()> {
        let mut inner = self.0.lock();
        if let std::task::Poll::Ready(x) = Pin::new(&mut inner.control).poll_close(c) {
            return Poll::Ready(x.map_err(YamuxError))
        }
        while let std::task::Poll::Ready(x) = inner.incoming.poll_next_unpin(c) {
            match x {
                Some(Ok(_))  => {} // drop inbound stream
                Some(Err(e)) => return Poll::Ready(Err(e)),
                None => return Poll::Ready(Ok(()))
            }
        }
        Poll::Pending
    }

    fn flush_all(&self, _: &mut Context<'_>) -> Poll<()> {
        Poll::Ready(Ok(()))
    }
}

/// The yamux configuration.
#[derive(Clone)]
pub struct Config {
    config: yamux::Config,
    mode: Option<yamux::Mode>
}

/// The yamux configuration for upgrading I/O resources which are ![`Send`].
#[derive(Clone)]
pub struct LocalConfig(Config);

impl Config {
    pub fn new(cfg: yamux::Config) -> Self {
        Config { config: cfg, mode: None }
    }

    /// Override the connection mode.
    ///
    /// This will always use the provided mode during the connection upgrade,
    /// irrespective of whether an inbound or outbound upgrade happens.
    pub fn override_mode(&mut self, mode: yamux::Mode) {
        self.mode = Some(mode)
    }

    /// Turn this into a [`LocalConfig`] for use with upgrades of ![`Send`] resources.
    pub fn local(self) -> LocalConfig {
        LocalConfig(self)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new(yamux::Config::default())
    }
}

impl Deref for Config {
    type Target = yamux::Config;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl DerefMut for Config {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.config
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
    type Output = Yamux<Incoming<C>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, io: C, _: Self::Info) -> Self::Future {
        future::ready(Ok(Yamux::new(io, self.config, self.mode.unwrap_or(yamux::Mode::Server))))
    }
}

impl<C> InboundUpgrade<C> for LocalConfig
where
    C: AsyncRead + AsyncWrite + Unpin + 'static
{
    type Output = Yamux<LocalIncoming<C>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, io: C, _: Self::Info) -> Self::Future {
        let cfg = self.0;
        future::ready(Ok(Yamux::local(io, cfg.config, cfg.mode.unwrap_or(yamux::Mode::Server))))
    }
}

impl<C> OutboundUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    type Output = Yamux<Incoming<C>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, io: C, _: Self::Info) -> Self::Future {
        future::ready(Ok(Yamux::new(io, self.config, self.mode.unwrap_or(yamux::Mode::Client))))
    }
}

impl<C> OutboundUpgrade<C> for LocalConfig
where
    C: AsyncRead + AsyncWrite + Unpin + 'static
{
    type Output = Yamux<LocalIncoming<C>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, io: C, _: Self::Info) -> Self::Future {
        let cfg = self.0;
        future::ready(Ok(Yamux::local(io, cfg.config, cfg.mode.unwrap_or(yamux::Mode::Client))))
    }
}

/// The Yamux [`libp2p_core::StreamMuxer`] error type.
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Incoming")
    }
}

/// The [`futures::stream::Stream`] of incoming substreams (`!Send`).
pub struct LocalIncoming<T> {
    stream: LocalBoxStream<'static, Result<yamux::Stream, YamuxError>>,
    _marker: std::marker::PhantomData<T>
}

impl<T> fmt::Debug for LocalIncoming<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LocalIncoming")
    }
}

impl<T> Stream for Incoming<T> {
    type Item = Result<yamux::Stream, YamuxError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        self.stream.as_mut().poll_next_unpin(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

impl<T> Unpin for Incoming<T> {
}

impl<T> Stream for LocalIncoming<T> {
    type Item = Result<yamux::Stream, YamuxError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        self.stream.as_mut().poll_next_unpin(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

impl<T> Unpin for LocalIncoming<T> {
}
