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

use futures::{prelude::*, future};
use muxing::StreamMuxer;
use std::io::{Error as IoError, Read, Write};
use tokio_io::{AsyncRead, AsyncWrite};

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
            &EitherOutput::First(ref a) => a.prepare_uninitialized_buffer(buf),
            &EitherOutput::Second(ref b) => b.prepare_uninitialized_buffer(buf),
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
            &mut EitherOutput::First(ref mut a) => a.read(buf),
            &mut EitherOutput::Second(ref mut b) => b.read(buf),
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
            &mut EitherOutput::First(ref mut a) => a.shutdown(),
            &mut EitherOutput::Second(ref mut b) => b.shutdown(),
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
            &mut EitherOutput::First(ref mut a) => a.write(buf),
            &mut EitherOutput::Second(ref mut b) => b.write(buf),
        }
    }

    #[inline]
    fn flush(&mut self) -> Result<(), IoError> {
        match self {
            &mut EitherOutput::First(ref mut a) => a.flush(),
            &mut EitherOutput::Second(ref mut b) => b.flush(),
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

    fn poll_inbound(&self) -> Poll<Option<Self::Substream>, IoError> {
        match *self {
            EitherOutput::First(ref inner) => inner.poll_inbound().map(|p| p.map(|o| o.map(EitherOutput::First))),
            EitherOutput::Second(ref inner) => inner.poll_inbound().map(|p| p.map(|o| o.map(EitherOutput::Second))),
        }
    }

    fn open_outbound(&self) -> Self::OutboundSubstream {
        match *self {
            EitherOutput::First(ref inner) => EitherOutbound::A(inner.open_outbound()),
            EitherOutput::Second(ref inner) => EitherOutbound::B(inner.open_outbound()),
        }
    }

    fn poll_outbound(&self, substream: &mut Self::OutboundSubstream) -> Poll<Option<Self::Substream>, IoError> {
        match (self, substream) {
            (EitherOutput::First(ref inner), EitherOutbound::A(ref mut substream)) => {
                inner.poll_outbound(substream).map(|p| p.map(|o| o.map(EitherOutput::First)))
            },
            (EitherOutput::Second(ref inner), EitherOutbound::B(ref mut substream)) => {
                inner.poll_outbound(substream).map(|p| p.map(|o| o.map(EitherOutput::Second)))
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {
        match *self {
            EitherOutput::First(ref inner) => {
                match substream {
                    EitherOutbound::A(substream) => inner.destroy_outbound(substream),
                    _ => panic!("Wrong API usage")
                }
            },
            EitherOutput::Second(ref inner) => {
                match substream {
                    EitherOutbound::B(substream) => inner.destroy_outbound(substream),
                    _ => panic!("Wrong API usage")
                }
            },
        }
    }

    fn read_substream(&self, substream: &mut Self::Substream, buf: &mut [u8]) -> Result<usize, IoError> {
        match (self, substream) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut substream)) => {
                inner.read_substream(substream, buf)
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut substream)) => {
                inner.read_substream(substream, buf)
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn write_substream(&self, substream: &mut Self::Substream, buf: &[u8]) -> Result<usize, IoError> {
        match (self, substream) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut substream)) => {
                inner.write_substream(substream, buf)
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut substream)) => {
                inner.write_substream(substream, buf)
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn flush_substream(&self, substream: &mut Self::Substream) -> Result<(), IoError> {
        match (self, substream) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut substream)) => {
                inner.flush_substream(substream)
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut substream)) => {
                inner.flush_substream(substream)
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn shutdown_substream(&self, substream: &mut Self::Substream) -> Poll<(), IoError> {
        match (self, substream) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut substream)) => {
                inner.shutdown_substream(substream)
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut substream)) => {
                inner.shutdown_substream(substream)
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn destroy_substream(&self, substream: Self::Substream) {
        match *self {
            EitherOutput::First(ref inner) => {
                match substream {
                    EitherOutput::First(substream) => inner.destroy_substream(substream),
                    _ => panic!("Wrong API usage")
                }
            },
            EitherOutput::Second(ref inner) => {
                match substream {
                    EitherOutput::Second(substream) => inner.destroy_substream(substream),
                    _ => panic!("Wrong API usage")
                }
            },
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum EitherOutbound<A: StreamMuxer, B: StreamMuxer> {
    A(A::OutboundSubstream),
    B(B::OutboundSubstream),
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

impl<A, B, Ao, Bo, Af, Bf> Future for EitherListenUpgrade<A, B>
where
    A: Future<Item = (Ao, Af), Error = IoError>,
    B: Future<Item = (Bo, Bf), Error = IoError>,
{
    type Item = (EitherOutput<Ao, Bo>, future::Either<Af, Bf>);
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self {
            &mut EitherListenUpgrade::First(ref mut a) => {
                let (item, addr) = try_ready!(a.poll());
                Ok(Async::Ready((EitherOutput::First(item), future::Either::A(addr))))
            }
            &mut EitherListenUpgrade::Second(ref mut b) => {
                let (item, addr) = try_ready!(b.poll());
                Ok(Async::Ready((EitherOutput::Second(item), future::Either::B(addr))))
            }
        }
    }
}
