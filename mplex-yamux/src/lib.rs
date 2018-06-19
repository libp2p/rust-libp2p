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
extern crate parking_lot;
extern crate tokio_executor;
extern crate tokio_io;
extern crate yamux;

use bytes::Bytes;
use core::Endpoint;
use futures::{future::{self, FutureResult}, prelude::*, sync::mpsc};
use parking_lot::Mutex;
use std::{io, iter, marker::PhantomData, sync::Arc};
use tokio_executor::{DefaultExecutor, Executor};
use tokio_io::{AsyncRead, AsyncWrite};


pub struct Yamux<C> {
    outgoing: yamux::Ctrl,
    incoming: Arc<Mutex<mpsc::UnboundedReceiver<yamux::Stream>>>,
    witness: PhantomData<*const C>
}

impl<C> Clone for Yamux<C> {
    fn clone(&self) -> Self {
        Yamux {
            outgoing: self.outgoing.clone(),
            incoming: self.incoming.clone(),
            witness: self.witness
        }
    }
}

impl<C> Yamux<C>
where
    C: AsyncRead + AsyncWrite + Send + 'static
{
    pub fn new(c: C, cfg: Arc<yamux::Config>, mode: yamux::Mode) -> Result<Self, io::Error> {
        let connection = yamux::Connection::new(c, cfg, mode);
        let ctrl = connection.control();
        let (conn_tx, conn_rx) = mpsc::unbounded();

        let future =
            connection.for_each(move |stream| {
                future::result(conn_tx.unbounded_send(stream))
                    .map_err(|_| yamux::error::ConnectionError::Closed)
            })
            .map_err(|e| error!("connection error: {}", e));

        DefaultExecutor::current().spawn(Box::new(future))
            .map_err(|e| {
                error!("failed to spawn future: {:?}", e);
                io::Error::new(io::ErrorKind::Other, "failed to spawn future")
            })?;

        Ok(Yamux {
            outgoing: ctrl,
            incoming: Arc::new(Mutex::new(conn_rx)),
            witness: PhantomData
        })
    }
}

impl<C> core::StreamMuxer for Yamux<C>
where
    C: AsyncRead + AsyncWrite + 'static
{
    type Substream = yamux::Stream;
    type InboundSubstream = Box<Future<Item=Option<Self::Substream>, Error=io::Error>>;
    type OutboundSubstream = Box<Future<Item=Option<Self::Substream>, Error=io::Error>>;

    fn inbound(self) -> Self::InboundSubstream {
        let incoming = self.incoming.clone();
        let future = future::poll_fn(move || {
            let mut rx = incoming.lock();
            match rx.poll() {
                Ok(Async::NotReady) => Ok(Async::NotReady),
                Ok(Async::Ready(None)) | Err(()) => Ok(Async::Ready(None)),
                Ok(Async::Ready(Some(stream))) => Ok(Async::Ready(Some(stream))),
            }
        });
        Box::new(future)
    }

    fn outbound(self) -> Self::OutboundSubstream {
        let future = self.outgoing.open_stream(None)
            .map(|stream| Some(stream))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e));
        Box::new(future)
    }
}


#[derive(Clone)]
pub struct YamuxConfig {
    config: Arc<yamux::Config>
}

impl YamuxConfig {
    pub fn new(cfg: yamux::Config) -> Self {
        YamuxConfig {
            config: Arc::new(cfg)
        }
    }
}

impl Default for YamuxConfig {
    fn default() -> Self {
        YamuxConfig::new(yamux::Config::default())
    }
}

impl<C, M> core::ConnectionUpgrade<C, M> for YamuxConfig
where
    C: AsyncRead + AsyncWrite + Send + 'static,
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
        let yamux = Yamux::new(i, self.config.clone(), mode).map(|yamux| (yamux, remote));
        future::result(yamux)
    }
}
