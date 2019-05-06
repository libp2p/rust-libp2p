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

//! Futures performing 1.5 round trips.

use crate::{
    io::{Handshake, NoiseOutput},
    NoiseError, NoiseSession,
};
use futures::prelude::*;
use std::mem;
use tokio_io::{AsyncRead, AsyncWrite};

/// A future for inbound upgrades.
///
/// It will perform the following steps:
///
/// 1. receive message
/// 2. send message
/// 3. receive message
pub struct NoiseInboundFuture<T> {
    state: InboundState<T>,
}

impl<T> NoiseInboundFuture<T> {
    pub(super) fn new(io: T, session: NoiseSession) -> Self {
        NoiseInboundFuture {
            state: InboundState::RecvHandshake1(Handshake::new(io, session)),
        }
    }
}

enum InboundState<T> {
    RecvHandshake1(Handshake<T>),
    SendHandshake(Handshake<T>),
    Flush(Handshake<T>),
    RecvHandshake2(Handshake<T>),
    Err(NoiseError),
    Done,
}

impl<T> Future for NoiseInboundFuture<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Item = ([u8; 32], NoiseOutput<T>);
    type Error = NoiseError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.state, InboundState::Done) {
                InboundState::RecvHandshake1(mut io) => {
                    if io.receive()?.is_ready() {
                        self.state = InboundState::SendHandshake(io)
                    } else {
                        self.state = InboundState::RecvHandshake1(io);
                        return Ok(Async::NotReady);
                    }
                }
                InboundState::SendHandshake(mut io) => {
                    if io.send()?.is_ready() {
                        self.state = InboundState::Flush(io)
                    } else {
                        self.state = InboundState::SendHandshake(io);
                        return Ok(Async::NotReady);
                    }
                }
                InboundState::Flush(mut io) => {
                    if io.flush()?.is_ready() {
                        self.state = InboundState::RecvHandshake2(io)
                    } else {
                        self.state = InboundState::Flush(io);
                        return Ok(Async::NotReady);
                    }
                }
                InboundState::RecvHandshake2(mut io) => {
                    if io.receive()?.is_ready() {
                        let result = io.finish()?;
                        self.state = InboundState::Done;
                        return Ok(Async::Ready(result));
                    } else {
                        self.state = InboundState::RecvHandshake2(io);
                        return Ok(Async::NotReady);
                    }
                }
                InboundState::Err(e) => return Err(e),
                InboundState::Done => panic!("NoiseInboundFuture::poll called after completion"),
            }
        }
    }
}

/// A future for outbound upgrades.
///
/// It will perform the following steps:
///
/// 1. send message
/// 2. receive message
/// 3. send message
pub struct NoiseOutboundFuture<T> {
    state: OutboundState<T>,
}

impl<T> NoiseOutboundFuture<T> {
    pub(super) fn new(io: T, session: NoiseSession) -> Self {
        NoiseOutboundFuture {
            state: OutboundState::SendHandshake1(Handshake::new(io, session)),
        }
    }
}

enum OutboundState<T> {
    SendHandshake1(Handshake<T>),
    Flush1(Handshake<T>),
    RecvHandshake(Handshake<T>),
    SendHandshake2(Handshake<T>),
    Flush2(Handshake<T>),
    Err(NoiseError),
    Done,
}

impl<T> Future for NoiseOutboundFuture<T>
where
    T: AsyncRead + AsyncWrite,
{
    type Item = ([u8; 32], NoiseOutput<T>);
    type Error = NoiseError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.state, OutboundState::Done) {
                OutboundState::SendHandshake1(mut io) => {
                    if io.send()?.is_ready() {
                        self.state = OutboundState::Flush1(io)
                    } else {
                        self.state = OutboundState::SendHandshake1(io);
                        return Ok(Async::NotReady);
                    }
                }
                OutboundState::Flush1(mut io) => {
                    if io.flush()?.is_ready() {
                        self.state = OutboundState::RecvHandshake(io)
                    } else {
                        self.state = OutboundState::Flush1(io);
                        return Ok(Async::NotReady);
                    }
                }
                OutboundState::RecvHandshake(mut io) => {
                    if io.receive()?.is_ready() {
                        self.state = OutboundState::SendHandshake2(io)
                    } else {
                        self.state = OutboundState::RecvHandshake(io);
                        return Ok(Async::NotReady);
                    }
                }
                OutboundState::SendHandshake2(mut io) => {
                    if io.send()?.is_ready() {
                        self.state = OutboundState::Flush2(io)
                    } else {
                        self.state = OutboundState::SendHandshake2(io);
                        return Ok(Async::NotReady);
                    }
                }
                OutboundState::Flush2(mut io) => {
                    if io.flush()?.is_ready() {
                        let result = io.finish()?;
                        self.state = OutboundState::Done;
                        return Ok(Async::Ready(result));
                    } else {
                        self.state = OutboundState::Flush2(io);
                        return Ok(Async::NotReady);
                    }
                }
                OutboundState::Err(e) => return Err(e),
                OutboundState::Done => panic!("NoiseOutboundFuture::poll called after completion"),
            }
        }
    }
}
