// Copyright 2017 Parity Technologies (UK) Ltd.
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

use futures::prelude::*;
use multiaddr::Multiaddr;
use muxing::StreamMuxer;
use std::io::{Error as IoError, Read, Write};
use tokio_io::{AsyncRead, AsyncWrite};

/// Implements `AsyncRead` and `AsyncWrite` and dispatches all method calls to
/// either `First` or `Second`.
#[derive(Debug, Copy, Clone)]
pub enum EitherSocket<A, B> {
    First(A),
    Second(B),
}

impl<A, B> AsyncRead for EitherSocket<A, B>
where
    A: AsyncRead,
    B: AsyncRead,
{
    #[inline]
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        match self {
            &EitherSocket::First(ref a) => a.prepare_uninitialized_buffer(buf),
            &EitherSocket::Second(ref b) => b.prepare_uninitialized_buffer(buf),
        }
    }
}

impl<A, B> Read for EitherSocket<A, B>
where
    A: Read,
    B: Read,
{
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        match self {
            &mut EitherSocket::First(ref mut a) => a.read(buf),
            &mut EitherSocket::Second(ref mut b) => b.read(buf),
        }
    }
}

impl<A, B> AsyncWrite for EitherSocket<A, B>
where
    A: AsyncWrite,
    B: AsyncWrite,
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), IoError> {
        match self {
            &mut EitherSocket::First(ref mut a) => a.shutdown(),
            &mut EitherSocket::Second(ref mut b) => b.shutdown(),
        }
    }
}

impl<A, B> Write for EitherSocket<A, B>
where
    A: Write,
    B: Write,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        match self {
            &mut EitherSocket::First(ref mut a) => a.write(buf),
            &mut EitherSocket::Second(ref mut b) => b.write(buf),
        }
    }

    #[inline]
    fn flush(&mut self) -> Result<(), IoError> {
        match self {
            &mut EitherSocket::First(ref mut a) => a.flush(),
            &mut EitherSocket::Second(ref mut b) => b.flush(),
        }
    }
}

impl<A, B> StreamMuxer for EitherSocket<A, B>
where
    A: StreamMuxer,
    B: StreamMuxer,
{
    type Substream = EitherSocket<A::Substream, B::Substream>;
    type InboundSubstream = EitherInbound<A, B>;
    type OutboundSubstream = EitherOutbound<A, B>;

    #[inline]
    fn inbound(self) -> Self::InboundSubstream {
        match self {
            EitherSocket::First(a) => EitherInbound::A(a.inbound()),
            EitherSocket::Second(b) => EitherInbound::B(b.inbound()),
        }
    }

    #[inline]
    fn outbound(self) -> Self::OutboundSubstream {
        match self {
            EitherSocket::First(a) => EitherOutbound::A(a.outbound()),
            EitherSocket::Second(b) => EitherOutbound::B(b.outbound()),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum EitherInbound<A: StreamMuxer, B: StreamMuxer> {
    A(A::InboundSubstream),
    B(B::InboundSubstream),
}

impl<A, B> Future for EitherInbound<A, B>
where
    A: StreamMuxer,
    B: StreamMuxer,
{
    type Item = Option<EitherSocket<A::Substream, B::Substream>>;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match *self {
            EitherInbound::A(ref mut a) => {
                let item = try_ready!(a.poll());
                Ok(Async::Ready(item.map(EitherSocket::First)))
            }
            EitherInbound::B(ref mut b) => {
                let item = try_ready!(b.poll());
                Ok(Async::Ready(item.map(EitherSocket::Second)))
            }
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum EitherOutbound<A: StreamMuxer, B: StreamMuxer> {
    A(A::OutboundSubstream),
    B(B::OutboundSubstream),
}

impl<A, B> Future for EitherOutbound<A, B>
where
    A: StreamMuxer,
    B: StreamMuxer,
{
    type Item = Option<EitherSocket<A::Substream, B::Substream>>;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match *self {
            EitherOutbound::A(ref mut a) => {
                let item = try_ready!(a.poll());
                Ok(Async::Ready(item.map(EitherSocket::First)))
            }
            EitherOutbound::B(ref mut b) => {
                let item = try_ready!(b.poll());
                Ok(Async::Ready(item.map(EitherSocket::Second)))
            }
        }
    }
}

/// Implements `Stream` and dispatches all method calls to either `First` or `Second`.
#[derive(Debug, Copy, Clone)]
pub enum EitherListenStream<A, B> {
    First(A),
    Second(B),
}

impl<AStream, BStream, AInner, BInner> Stream for EitherListenStream<AStream, BStream>
where
    AStream: Stream<Item = AInner, Error = IoError>,
    BStream: Stream<Item = BInner, Error = IoError>,
{
    type Item = EitherListenUpgrade<AInner, BInner>;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self {
            &mut EitherListenStream::First(ref mut a) => a.poll()
                .map(|i| i.map(|v| v.map(EitherListenUpgrade::First))),
            &mut EitherListenStream::Second(ref mut a) => a.poll()
                .map(|i| i.map(|v| v.map(EitherListenUpgrade::Second))),
        }
    }
}

// TODO: This type is needed because of the lack of `impl Trait` in stable Rust.
//         If Rust had impl Trait we could use the Either enum from the futures crate and add some
//         modifiers to it. This custom enum is a combination of Either and these modifiers.
#[derive(Debug, Copy, Clone)]
pub enum EitherListenUpgrade<A, B> {
    First(A),
    Second(B),
}

impl<A, B, Ao, Bo> Future for EitherListenUpgrade<A, B>
where
    A: Future<Item = (Ao, Multiaddr), Error = IoError>,
    B: Future<Item = (Bo, Multiaddr), Error = IoError>,
{
    type Item = (EitherSocket<Ao, Bo>, Multiaddr);
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self {
            &mut EitherListenUpgrade::First(ref mut a) => {
                let (item, addr) = try_ready!(a.poll());
                Ok(Async::Ready((EitherSocket::First(item), addr)))
            }
            &mut EitherListenUpgrade::Second(ref mut b) => {
                let (item, addr) = try_ready!(b.poll());
                Ok(Async::Ready((EitherSocket::Second(item), addr)))
            }
        }
    }
}
