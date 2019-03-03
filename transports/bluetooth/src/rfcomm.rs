// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::{Addr, profile_register, rfcomm_socket::RfcommSocket};
use futures::{prelude::*, try_ready};
use std::{ffi::CStr, io, mem};

pub struct RfcommStream {
    inner: tokio_reactor::PollEvented<RfcommSocket>,
}

impl RfcommStream {
    pub fn connect(dest: Addr, port: u8) -> RfcommStreamFuture {
        let socket = match RfcommSocket::connect(dest, port) {
            Ok(s) => s,
            Err(err) => return RfcommStreamFuture {
                inner: RfcommStreamFutureInner::Error(err)
            },
        };

        RfcommStreamFuture {
            inner: RfcommStreamFutureInner::Waiting(tokio_reactor::PollEvented::new(socket))
        }
    }
}

pub struct RfcommStreamFuture {
    inner: RfcommStreamFutureInner,
}

enum RfcommStreamFutureInner {
    Waiting(tokio_reactor::PollEvented<RfcommSocket>),
    Error(io::Error),
    Finished,
}

impl Future for RfcommStreamFuture {
    type Item = RfcommStream;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match mem::replace(&mut self.inner, RfcommStreamFutureInner::Finished) {
            RfcommStreamFutureInner::Waiting(socket) => match socket.poll_write_ready() {
                Ok(Async::Ready(_)) => {
                    Ok(Async::Ready(RfcommStream {
                        inner: socket,
                    }))
                }
                Ok(Async::NotReady) => {
                    self.inner = RfcommStreamFutureInner::Waiting(socket);
                    Ok(Async::NotReady)
                }
                Err(err) => Err(err),
            },
            RfcommStreamFutureInner::Error(err) => Err(err),
            RfcommStreamFutureInner::Finished => panic!("future polled after finished"),
        }
    }
}

impl io::Read for RfcommStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.inner.read(buf)
    }
}

impl tokio_io::AsyncRead for RfcommStream {
    // TODO: specialize functions
}

impl io::Write for RfcommStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        self.inner.flush()
    }
}

impl tokio_io::AsyncWrite for RfcommStream {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.inner.shutdown()
    }
}

pub struct RfcommListener {
    inner: tokio_reactor::PollEvented<RfcommSocket>,
    sdp_registration: Option<profile_register::Registration>,
}

impl RfcommListener {
    pub fn bind(addr: Addr, port: u8) -> Result<(RfcommListener, u8), io::Error> {
        crate::discoverable::enable_discoverable(&addr)?;

        let (socket, actual_port) = if port != 0 {
            (RfcommSocket::bind(addr, port)?, port)
        } else {
            (1..30)
                .filter_map(|port| {
                    RfcommSocket::bind(addr, port).ok().map(|s| (s, port))
                })
                .next()
                .ok_or_else(|| io::Error::last_os_error())?
        };

        let sdp_registration = profile_register::register_libp2p_profile(actual_port)
            .map_err(|err| { println!("registration error: {:?}", err); err }).ok();

        let inner = RfcommListener {
            inner: tokio_reactor::PollEvented::new(socket),
            sdp_registration,
        };

        Ok((inner, actual_port))
    }
}

impl Stream for RfcommListener {
    type Item = (RfcommStream, Addr);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let ready = mio::Ready::readable();
        try_ready!(self.inner.poll_read_ready(ready));

        match self.inner.get_ref().accept() {
            Ok((client, addr)) => {
                let stream = RfcommStream {
                    inner: tokio_reactor::PollEvented::new(client)
                };
                Ok(Async::Ready(Some((stream, addr))))
            },
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.inner.clear_read_ready(ready)?;
                Ok(Async::NotReady)
            }
            Err(e) => Err(e),
        }
    }
}
