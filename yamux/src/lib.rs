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

extern crate bytes;
extern crate futures;
#[macro_use]
extern crate log;
extern crate libp2p_core as core;
extern crate tokio_io;
extern crate yamux;

use bytes::Bytes;
use core::Endpoint;
use futures::{future::{self, FutureResult}, prelude::*};
use std::{io, iter};
use tokio_io::{AsyncRead, AsyncWrite};


pub struct Yamux<C>(yamux::Connection<C>);

impl<C> Clone for Yamux<C> {
    fn clone(&self) -> Self {
        Yamux(self.0.clone())
    }
}

impl<C> Yamux<C>
where
    C: AsyncRead + AsyncWrite + 'static
{
    pub fn new(c: C, cfg: yamux::Config, mode: yamux::Mode) -> Self {
        Yamux(yamux::Connection::new(c, cfg, mode))
    }
}

impl<C> core::StreamMuxer for Yamux<C>
where
    C: AsyncRead + AsyncWrite + 'static
{
    type Substream = yamux::StreamHandle<C>;
    type InboundSubstream = InboundFuture<C>;
    type OutboundSubstream = FutureResult<Option<Self::Substream>, io::Error>;

    fn inbound(self) -> Self::InboundSubstream {
        InboundFuture(self.0)
    }

    fn outbound(self) -> Self::OutboundSubstream {
        let stream = self.0.open_stream().map_err(|e| io::Error::new(io::ErrorKind::Other, e));
        future::result(stream)
    }
}


pub struct InboundFuture<C>(yamux::Connection<C>);

impl<C> Future for InboundFuture<C>
where
    C: AsyncRead + AsyncWrite + 'static
{
    type Item = Option<yamux::StreamHandle<C>>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.0.poll() {
            Err(e) => {
                error!("connection error: {}", e);
                Err(io::Error::new(io::ErrorKind::Other, e))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Ok(Async::Ready(Some(stream))) => Ok(Async::Ready(Some(stream)))
        }
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

impl<C, M> core::ConnectionUpgrade<C, M> for Config
where
    C: AsyncRead + AsyncWrite + 'static,
    M: 'static
{
    type UpgradeIdentifier = ();
    type NamesIter = iter::Once<(Bytes, ())>;

    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/yamux/1.0.0"), ()))
    }

    type Output = Yamux<C>;
    type MultiaddrFuture = M;
    type Future = FutureResult<(Yamux<C>, M), io::Error>;

    fn upgrade(self, i: C, _: (), end: Endpoint, remote: M) -> Self::Future {
        let mode = match end {
            Endpoint::Dialer => yamux::Mode::Client,
            Endpoint::Listener => yamux::Mode::Server
        };
        future::ok((Yamux::new(i, self.0, mode), remote))
    }
}

