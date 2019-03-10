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

use futures::{future::{self, FutureResult}, prelude::*};
use libp2p_core::{muxing::Shutdown, upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo}};
use log::error;
use std::{io, iter, sync::atomic};
use std::io::{Error as IoError};
use tokio_io::{AsyncRead, AsyncWrite};

// TODO: add documentation and field names
pub struct Yamux<C>(yamux::Connection<C>, atomic::AtomicBool);

impl<C> Yamux<C>
where
    C: AsyncRead + AsyncWrite + 'static
{
    pub fn new(c: C, cfg: yamux::Config, mode: yamux::Mode) -> Self {
        Yamux(yamux::Connection::new(c, cfg, mode), atomic::AtomicBool::new(false))
    }
}

impl<C> libp2p_core::StreamMuxer for Yamux<C>
where
    C: AsyncRead + AsyncWrite + 'static
{
    type Substream = yamux::StreamHandle<C>;
    type OutboundSubstream = FutureResult<Option<Self::Substream>, io::Error>;

    #[inline]
    fn poll_inbound(&self) -> Poll<Option<Self::Substream>, IoError> {
        match self.0.poll() {
            Err(e) => {
                error!("connection error: {}", e);
                Err(io::Error::new(io::ErrorKind::Other, e))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Ok(Async::Ready(Some(stream))) => {
                self.1.store(true, atomic::Ordering::Release);
                Ok(Async::Ready(Some(stream)))
            }
        }
    }

    #[inline]
    fn open_outbound(&self) -> Self::OutboundSubstream {
        let stream = self.0.open_stream().map_err(|e| io::Error::new(io::ErrorKind::Other, e));
        future::result(stream)
    }

    #[inline]
    fn poll_outbound(&self, substream: &mut Self::OutboundSubstream) -> Poll<Option<Self::Substream>, IoError> {
        substream.poll()
    }

    #[inline]
    fn destroy_outbound(&self, _: Self::OutboundSubstream) {
    }

    #[inline]
    fn read_substream(&self, sub: &mut Self::Substream, buf: &mut [u8]) -> Poll<usize, IoError> {
        let result = sub.poll_read(buf);
        if let Ok(Async::Ready(_)) = result {
            self.1.store(true, atomic::Ordering::Release);
        }
        result
    }

    #[inline]
    fn write_substream(&self, sub: &mut Self::Substream, buf: &[u8]) -> Poll<usize, IoError> {
        sub.poll_write(buf)
    }

    #[inline]
    fn flush_substream(&self, sub: &mut Self::Substream) -> Poll<(), IoError> {
        sub.poll_flush()
    }

    #[inline]
    fn shutdown_substream(&self, sub: &mut Self::Substream, _: Shutdown) -> Poll<(), IoError> {
        sub.shutdown()
    }

    #[inline]
    fn destroy_substream(&self, _: Self::Substream) {
    }

    #[inline]
    fn is_remote_acknowledged(&self) -> bool {
        self.1.load(atomic::Ordering::Acquire)
    }

    #[inline]
    fn shutdown(&self, _: Shutdown) -> Poll<(), IoError> {
        self.0.close()
    }

    #[inline]
    fn flush_all(&self) -> Poll<(), IoError> {
        self.0.flush()
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
    C: AsyncRead + AsyncWrite + 'static,
{
    type Output = Yamux<C>;
    type Error = io::Error;
    type Future = FutureResult<Yamux<C>, io::Error>;

    fn upgrade_inbound(self, i: C, _: Self::Info) -> Self::Future {
        future::ok(Yamux::new(i, self.0, yamux::Mode::Server))
    }
}

impl<C> OutboundUpgrade<C> for Config
where
    C: AsyncRead + AsyncWrite + 'static,
{
    type Output = Yamux<C>;
    type Error = io::Error;
    type Future = FutureResult<Yamux<C>, io::Error>;

    fn upgrade_outbound(self, i: C, _: Self::Info) -> Self::Future {
        future::ok(Yamux::new(i, self.0, yamux::Mode::Client))
    }
}

