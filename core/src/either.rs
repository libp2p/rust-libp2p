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

use crate::{muxing::StreamMuxer, Multiaddr, ProtocolName};
use futures::prelude::*;
use std::{fmt, io::{Error as IoError, Read, Write}};
use tokio_io::{AsyncRead, AsyncWrite};

#[derive(Debug, Copy, Clone)]
pub enum EitherError<A, B> {
    A(A),
    B(B)
}

impl<A, B> fmt::Display for EitherError<A, B>
where
    A: fmt::Display,
    B: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EitherError::A(a) => a.fmt(f),
            EitherError::B(b) => b.fmt(f)
        }
    }
}

impl<A, B> std::error::Error for EitherError<A, B>
where
    A: fmt::Debug + std::error::Error,
    B: fmt::Debug + std::error::Error
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EitherError::A(a) => a.source(),
            EitherError::B(b) => b.source()
        }
    }
}

/// Implements `AsyncRead` and `AsyncWrite` and dispatches all method calls to
/// either `First` or `Second`.
#[derive(Debug, Copy, Clone)]
pub enum EitherOutput<A, B> {
    First(A),
    Second(B),
}

impl<A, B> AsyncRead for EitherOutput<A, B>
where
    A: AsyncRead,
    B: AsyncRead,
{
    #[inline]
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        match self {
            EitherOutput::First(a) => a.prepare_uninitialized_buffer(buf),
            EitherOutput::Second(b) => b.prepare_uninitialized_buffer(buf),
        }
    }
}

impl<A, B> Read for EitherOutput<A, B>
where
    A: Read,
    B: Read,
{
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        match self {
            EitherOutput::First(a) => a.read(buf),
            EitherOutput::Second(b) => b.read(buf),
        }
    }
}

impl<A, B> AsyncWrite for EitherOutput<A, B>
where
    A: AsyncWrite,
    B: AsyncWrite,
{
    #[inline]
    fn shutdown(&mut self) -> Poll<(), IoError> {
        match self {
            EitherOutput::First(a) => a.shutdown(),
            EitherOutput::Second(b) => b.shutdown(),
        }
    }
}

impl<A, B> Write for EitherOutput<A, B>
where
    A: Write,
    B: Write,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        match self {
            EitherOutput::First(a) => a.write(buf),
            EitherOutput::Second(b) => b.write(buf),
        }
    }

    #[inline]
    fn flush(&mut self) -> Result<(), IoError> {
        match self {
            EitherOutput::First(a) => a.flush(),
            EitherOutput::Second(b) => b.flush(),
        }
    }
}

impl<A, B> StreamMuxer for EitherOutput<A, B>
where
    A: StreamMuxer,
    B: StreamMuxer,
{
    type Substream = EitherOutput<A::Substream, B::Substream>;
    type OutboundSubstream = EitherOutbound<A, B>;

    fn poll_inbound(&self) -> Poll<Self::Substream, IoError> {
        match self {
            EitherOutput::First(inner) => inner.poll_inbound().map(|p| p.map(EitherOutput::First)),
            EitherOutput::Second(inner) => inner.poll_inbound().map(|p| p.map(EitherOutput::Second)),
        }
    }

    fn open_outbound(&self) -> Self::OutboundSubstream {
        match self {
            EitherOutput::First(inner) => EitherOutbound::A(inner.open_outbound()),
            EitherOutput::Second(inner) => EitherOutbound::B(inner.open_outbound()),
        }
    }

    fn poll_outbound(&self, substream: &mut Self::OutboundSubstream) -> Poll<Self::Substream, IoError> {
        match (self, substream) {
            (EitherOutput::First(ref inner), EitherOutbound::A(ref mut substream)) => {
                inner.poll_outbound(substream).map(|p| p.map(EitherOutput::First))
            },
            (EitherOutput::Second(ref inner), EitherOutbound::B(ref mut substream)) => {
                inner.poll_outbound(substream).map(|p| p.map(EitherOutput::Second))
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {
        match self {
            EitherOutput::First(inner) => {
                match substream {
                    EitherOutbound::A(substream) => inner.destroy_outbound(substream),
                    _ => panic!("Wrong API usage")
                }
            },
            EitherOutput::Second(inner) => {
                match substream {
                    EitherOutbound::B(substream) => inner.destroy_outbound(substream),
                    _ => panic!("Wrong API usage")
                }
            },
        }
    }

    fn read_substream(&self, sub: &mut Self::Substream, buf: &mut [u8]) -> Poll<usize, IoError> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.read_substream(sub, buf)
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.read_substream(sub, buf)
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn write_substream(&self, sub: &mut Self::Substream, buf: &[u8]) -> Poll<usize, IoError> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.write_substream(sub, buf)
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.write_substream(sub, buf)
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn flush_substream(&self, sub: &mut Self::Substream) -> Poll<(), IoError> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.flush_substream(sub)
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.flush_substream(sub)
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn shutdown_substream(&self, sub: &mut Self::Substream) -> Poll<(), IoError> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.shutdown_substream(sub)
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.shutdown_substream(sub)
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn destroy_substream(&self, substream: Self::Substream) {
        match self {
            EitherOutput::First(inner) => {
                match substream {
                    EitherOutput::First(substream) => inner.destroy_substream(substream),
                    _ => panic!("Wrong API usage")
                }
            },
            EitherOutput::Second(inner) => {
                match substream {
                    EitherOutput::Second(substream) => inner.destroy_substream(substream),
                    _ => panic!("Wrong API usage")
                }
            },
        }
    }

    fn is_remote_acknowledged(&self) -> bool {
        match self {
            EitherOutput::First(inner) => inner.is_remote_acknowledged(),
            EitherOutput::Second(inner) => inner.is_remote_acknowledged()
        }
    }

    fn close(&self) -> Poll<(), IoError> {
        match self {
            EitherOutput::First(inner) => inner.close(),
            EitherOutput::Second(inner) => inner.close()
        }
    }

    fn flush_all(&self) -> Poll<(), IoError> {
        match self {
            EitherOutput::First(inner) => inner.flush_all(),
            EitherOutput::Second(inner) => inner.flush_all()
        }
    }
}

#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherOutbound<A: StreamMuxer, B: StreamMuxer> {
    A(A::OutboundSubstream),
    B(B::OutboundSubstream),
}

/// Implements `Stream` and dispatches all method calls to either `First` or `Second`.
#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherListenStream<A, B> {
    First(A),
    Second(B),
}

impl<AStream, BStream, AInner, BInner> Stream for EitherListenStream<AStream, BStream>
where
    AStream: Stream<Item = (AInner, Multiaddr)>,
    BStream: Stream<Item = (BInner, Multiaddr)>,
{
    type Item = (EitherFuture<AInner, BInner>, Multiaddr);
    type Error = EitherError<AStream::Error, BStream::Error>;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self {
            EitherListenStream::First(a) => a.poll()
                .map(|i| (i.map(|v| (v.map(|(o, addr)| (EitherFuture::First(o), addr))))))
                .map_err(EitherError::A),
            EitherListenStream::Second(a) => a.poll()
                .map(|i| (i.map(|v| (v.map(|(o, addr)| (EitherFuture::Second(o), addr))))))
                .map_err(EitherError::B),
        }
    }
}

/// Implements `Future` and dispatches all method calls to either `First` or `Second`.
#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherFuture<A, B> {
    First(A),
    Second(B),
}

impl<AFuture, BFuture, AInner, BInner> Future for EitherFuture<AFuture, BFuture>
where
    AFuture: Future<Item = AInner>,
    BFuture: Future<Item = BInner>,
{
    type Item = EitherOutput<AInner, BInner>;
    type Error = EitherError<AFuture::Error, BFuture::Error>;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self {
            EitherFuture::First(a) => a.poll().map(|v| v.map(EitherOutput::First)).map_err(EitherError::A),
            EitherFuture::Second(a) => a.poll().map(|v| v.map(EitherOutput::Second)).map_err(EitherError::B),
        }
    }
}

#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherFuture2<A, B> { A(A), B(B) }

impl<AFut, BFut, AItem, BItem, AError, BError> Future for EitherFuture2<AFut, BFut>
where
    AFut: Future<Item = AItem, Error = AError>,
    BFut: Future<Item = BItem, Error = BError>
{
    type Item = EitherOutput<AItem, BItem>;
    type Error = EitherError<AError, BError>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self {
            EitherFuture2::A(a) => a.poll()
                .map(|v| v.map(EitherOutput::First))
                .map_err(EitherError::A),

            EitherFuture2::B(b) => b.poll()
                .map(|v| v.map(EitherOutput::Second))
                .map_err(EitherError::B)
        }
    }
}

#[derive(Debug, Clone)]
pub enum EitherName<A, B> { A(A), B(B) }

impl<A: ProtocolName, B: ProtocolName> ProtocolName for EitherName<A, B> {
    fn protocol_name(&self) -> &[u8] {
        match self {
            EitherName::A(a) => a.protocol_name(),
            EitherName::B(b) => b.protocol_name()
        }
    }
}
