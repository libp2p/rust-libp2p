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

extern crate aio_limited;
#[macro_use]
extern crate futures;
extern crate libp2p_core as swarm;
#[macro_use]
extern crate log;
extern crate tokio;
extern crate tokio_io;

use aio_limited::{Limited, Limiter};
use std::io;
use swarm::{Multiaddr, Transport};
use tokio::executor::Executor;
use tokio::prelude::*;
use tokio_io::io::{ReadHalf, WriteHalf};

#[derive(Clone)]
pub struct RateLimited<T> {
    value: T,
    rlimiter: Limiter,
    wlimiter: Limiter,
}

impl<T> RateLimited<T> {
    pub fn new<E: Executor>(
        e: &mut E,
        value: T,
        max_read: usize,
        max_write: usize,
    ) -> io::Result<RateLimited<T>> {
        Ok(RateLimited {
            value,
            rlimiter: Limiter::new(e, max_read).map_err(|e| {
                error!("failed to create read limiter: {}", e);
                io::Error::new(io::ErrorKind::Other, e)
            })?,
            wlimiter: Limiter::new(e, max_write).map_err(|e| {
                error!("failed to create write limiter: {}", e);
                io::Error::new(io::ErrorKind::Other, e)
            })?,
        })
    }

    fn from_parts(value: T, r: Limiter, w: Limiter) -> RateLimited<T> {
        RateLimited {
            value,
            rlimiter: r,
            wlimiter: w,
        }
    }
}

/// A rate-limited connection.
pub struct Connection<C: AsyncRead + AsyncWrite> {
    reader: Limited<ReadHalf<C>>,
    writer: Limited<WriteHalf<C>>,
}

impl<C: AsyncRead + AsyncWrite> Connection<C> {
    pub fn new(c: C, rlimiter: Limiter, wlimiter: Limiter) -> io::Result<Connection<C>> {
        let (r, w) = c.split();
        Ok(Connection {
            reader: Limited::new(r, rlimiter).map_err(|e| {
                error!("failed to create limited reader: {}", e);
                io::Error::new(io::ErrorKind::Other, e)
            })?,
            writer: Limited::new(w, wlimiter).map_err(|e| {
                error!("failed to create limited writer: {}", e);
                io::Error::new(io::ErrorKind::Other, e)
            })?,
        })
    }
}

impl<C: AsyncRead + AsyncWrite> io::Read for Connection<C> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

impl<C: AsyncRead + AsyncWrite> io::Write for Connection<C> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl<C: AsyncRead + AsyncWrite> AsyncRead for Connection<C> {}

impl<C: AsyncRead + AsyncWrite> AsyncWrite for Connection<C> {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.writer.shutdown()
    }
}

pub struct Listener<T: Transport>(RateLimited<T::Listener>);

impl<T: Transport> Stream for Listener<T> {
    type Item = ListenerUpgrade<T>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match try_ready!(self.0.value.poll()) {
            Some(upgrade) => {
                let r = self.0.rlimiter.clone();
                let w = self.0.wlimiter.clone();
                let u = ListenerUpgrade(RateLimited::from_parts(upgrade, r, w));
                Ok(Async::Ready(Some(u)))
            }
            None => Ok(Async::Ready(None)),
        }
    }
}

pub struct ListenerUpgrade<T: Transport>(RateLimited<T::ListenerUpgrade>);

impl<T> Future for ListenerUpgrade<T>
where
    T: Transport + 'static,
    T::Output: AsyncRead + AsyncWrite,
{
    type Item = (Connection<T::Output>, Multiaddr);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (conn, addr) = try_ready!(self.0.value.poll());
        let r = self.0.rlimiter.clone();
        let w = self.0.wlimiter.clone();
        Ok(Async::Ready((Connection::new(conn, r, w)?, addr)))
    }
}

impl<T> Transport for RateLimited<T>
where
    T: Transport + 'static,
    T::Output: AsyncRead + AsyncWrite,
{
    type Output = Connection<T::Output>;
    type Listener = Listener<T>;
    type ListenerUpgrade = ListenerUpgrade<T>;
    type Dial = Box<Future<Item = (Connection<T::Output>, Multiaddr), Error = io::Error>>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)>
    where
        Self: Sized,
    {
        let r = self.rlimiter;
        let w = self.wlimiter;
        self.value
            .listen_on(addr)
            .map(|(listener, a)| {
                (
                    Listener(RateLimited::from_parts(listener, r.clone(), w.clone())),
                    a,
                )
            })
            .map_err(|(transport, a)| (RateLimited::from_parts(transport, r, w), a))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)>
    where
        Self: Sized,
    {
        let r = self.rlimiter;
        let w = self.wlimiter;
        let r2 = r.clone();
        let w2 = w.clone();

        self.value
            .dial(addr)
            .map(move |dial| {
                let future = dial
                    .and_then(move |(conn, addr)| Ok((Connection::new(conn, r, w)?, addr)));
                Box::new(future) as Box<_>
            })
            .map_err(|(transport, a)| (RateLimited::from_parts(transport, r2, w2), a))
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.value.nat_traversal(server, observed)
    }
}
