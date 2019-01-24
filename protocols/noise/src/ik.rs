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

use crate::{
    error::NoiseError,
    io::NoiseOutput,
    keys::{Curve25519, PublicKey},
    util::{to_array, Resolver}
};
use futures::prelude::*;
use snow;
use std::mem;
use tokio_io::{AsyncRead, AsyncWrite};

#[derive(Debug, Clone)]
pub enum IK {}

pub struct NoiseInboundFuture<T>(InboundState<T>);

impl<T> NoiseInboundFuture<T> {
    pub(super) fn new(io: T, c: super::NoiseConfig<IK>) -> Self {
        Self(InboundState::Init(io, c))
    }
}

enum InboundState<T> {
    Init(T, super::NoiseConfig<IK>),
    RecvHandshake(NoiseOutput<T>), // -> e, es, s, ss
    SendHandshake(NoiseOutput<T>), // <- e, ee, se
    Flush(NoiseOutput<T>),
    Done
}

impl<T> Future for NoiseInboundFuture<T>
where
    T: AsyncRead + AsyncWrite
{
    type Item = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.0, InboundState::Done) {
                InboundState::Init(io, config) => {
                    let session = snow::Builder::with_resolver(config.params, Box::new(Resolver))
                        .local_private_key(config.keypair.secret().as_ref())
                        .build_responder()?;
                    let output = NoiseOutput::new(io, session);
                    self.0 = InboundState::RecvHandshake(output)
                }
                InboundState::RecvHandshake(mut io) => {
                    if io.poll_read(&mut [])?.is_ready() {
                        self.0 = InboundState::SendHandshake(io)
                    } else {
                        self.0 = InboundState::RecvHandshake(io);
                        return Ok(Async::NotReady)
                    }
                }
                InboundState::SendHandshake(mut io) => {
                    if io.poll_write(&[])?.is_ready() {
                        self.0 = InboundState::Flush(io)
                    } else {
                        self.0 = InboundState::SendHandshake(io);
                        return Ok(Async::NotReady)
                    }
                }
                InboundState::Flush(mut io) => {
                    if io.poll_flush()?.is_ready() {
                        let s = io.session.into_transport_mode()?;
                        let m = s.get_remote_static()
                            .ok_or(NoiseError::InvalidKey)
                            .and_then(to_array)?;
                        let io = NoiseOutput { session: s, .. io };
                        self.0 = InboundState::Done;
                        return Ok(Async::Ready((PublicKey::new(m), io)))
                    } else {
                        self.0 = InboundState::Flush(io);
                        return Ok(Async::NotReady)
                    }
                }
                InboundState::Done => panic!("NoiseInboundFuture::poll called after completion")
            }
        }
    }
}

pub struct NoiseOutboundFuture<T>(OutboundState<T>);

impl<T> NoiseOutboundFuture<T> {
    pub(super) fn new(io: T, c: super::NoiseConfig<IK, PublicKey<Curve25519>>) -> Self {
        Self(OutboundState::Init(io, c))
    }
}

enum OutboundState<T> {
    Init(T, super::NoiseConfig<IK, PublicKey<Curve25519>>),
    SendHandshake(NoiseOutput<T>), // -> e, es, s, ss
    Flush(NoiseOutput<T>),
    RecvHandshake(NoiseOutput<T>), // <- e, ee, se
    Done
}

impl<T> Future for NoiseOutboundFuture<T>
where
    T: AsyncRead + AsyncWrite
{
    type Item = (PublicKey<Curve25519>, NoiseOutput<T>);
    type Error = NoiseError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.0, OutboundState::Done) {
                OutboundState::Init(io, config) => {
                    let session = snow::Builder::with_resolver(config.params, Box::new(Resolver))
                        .local_private_key(config.keypair.secret().as_ref())
                        .remote_public_key(config.remote.as_ref())
                        .build_initiator()?;
                    let output = NoiseOutput::new(io, session);
                    self.0 = OutboundState::SendHandshake(output)
                }
                OutboundState::SendHandshake(mut io) => {
                    if io.poll_write(&[])?.is_ready() {
                        self.0 = OutboundState::Flush(io)
                    } else {
                        self.0 = OutboundState::SendHandshake(io);
                        return Ok(Async::NotReady)
                    }
                }
                OutboundState::Flush(mut io) => {
                    if io.poll_flush()?.is_ready() {
                        self.0 = OutboundState::RecvHandshake(io)
                    } else {
                        self.0 = OutboundState::Flush(io);
                        return Ok(Async::NotReady)
                    }
                }
                OutboundState::RecvHandshake(mut io) => {
                    if io.poll_read(&mut [])?.is_ready() {
                        let s = io.session.into_transport_mode()?;
                        let m = s.get_remote_static()
                            .ok_or(NoiseError::InvalidKey)
                            .and_then(to_array)?;
                        let io = NoiseOutput { session: s, .. io };
                        self.0 = OutboundState::Done;
                        return Ok(Async::Ready((PublicKey::new(m), io)))
                    } else {
                        self.0 = OutboundState::RecvHandshake(io);
                        return Ok(Async::NotReady)
                    }
                }
                OutboundState::Done => panic!("NoiseOutboundFuture::poll called after completion")
            }
        }
    }
}
