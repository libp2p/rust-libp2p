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

use futures::{future, prelude::*, ready};
use libp2p_core::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo, Negotiated};
use parking_lot::Mutex;
use std::{fmt, io, iter, pin::Pin, task::Context};
use thiserror::Error;

/// A Yamux connection.
pub struct Yamux<C>(Mutex<Inner>, std::marker::PhantomData<C>);

struct Inner {
    /// The actual connection object as a `futures::stream::Stream`.
    connection: Pin<Box<dyn Stream<Item = Result<yamux::Stream, YamuxError>> + Send>>,
    /// Handle to control the connection.
    control: yamux::Control,
    /// True, once we have received an inbound substream.
    acknowledged: bool
}

impl<C> fmt::Debug for Yamux<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Yamux")
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

/// A token to poll for an outbound substream.
#[derive(Debug)]
pub struct OpenSubstreamToken(());

impl<C> Yamux<C>
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    /// Create a new Yamux connection.
    pub fn new(io: C, mut cfg: yamux::Config, mode: yamux::Mode) -> Self {
        cfg.set_read_after_close(false);
        let conn = yamux::Connection::new(io, cfg, mode);
        let ctrl = conn.control();
        let inner = Inner {
            connection: Box::pin(yamux::into_stream(conn).err_into()),
            control: ctrl,
            acknowledged: false
        };
        Yamux(Mutex::new(inner), std::marker::PhantomData)
    }
}

type Poll<T> = std::task::Poll<Result<T, YamuxError>>;

impl<C> libp2p_core::StreamMuxer for Yamux<C>
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    type Substream = yamux::Stream;
    type OutboundSubstream = OpenSubstreamToken;
    type Error = YamuxError;

    fn poll_inbound(&self, c: &mut Context) -> Poll<Self::Substream> {
        let mut inner = self.0.lock();
        match ready!(inner.connection.poll_next_unpin(c)) {
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

#[derive(Clone)]
pub struct Config(yamux::Config);

impl Config {
    pub fn new(cfg: yamux::Config) -> Self {
        Config(cfg)
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

impl<C> InboundUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    type Output = Yamux<Negotiated<C>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Yamux<Negotiated<C>>, Self::Error>>;

    fn upgrade_inbound(self, io: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ready(Ok(Yamux::new(io, self.0, yamux::Mode::Server)))
    }
}

impl<C> OutboundUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    type Output = Yamux<Negotiated<C>>;
    type Error = io::Error;
    type Future = future::Ready<Result<Yamux<Negotiated<C>>, Self::Error>>;

    fn upgrade_outbound(self, io: Negotiated<C>, _: Self::Info) -> Self::Future {
        future::ready(Ok(Yamux::new(io, self.0, yamux::Mode::Client)))
    }
}

