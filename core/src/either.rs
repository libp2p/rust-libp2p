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

use crate::{muxing::StreamMuxer, ProtocolName, transport::ListenerEvent};
use futures::prelude::*;
use std::{fmt, io::{Error as IoError, Read, Write}, pin::Pin, task::Context, task::Poll};

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
    A: std::error::Error,
    B: std::error::Error
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
    A: AsyncRead + Unpin,
    B: AsyncRead + Unpin,
{
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8]) -> Poll<Result<usize, IoError>> {
        match &mut *self {
            EitherOutput::First(a) => AsyncRead::poll_read(Pin::new(a), cx, buf),
            EitherOutput::Second(b) => AsyncRead::poll_read(Pin::new(b), cx, buf),
        }
    }
}

// TODO: remove?
impl<A, B> Read for EitherOutput<A, B>
where
    A: Read,
    B: Read,
{
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        match self {
            EitherOutput::First(a) => a.read(buf),
            EitherOutput::Second(b) => b.read(buf),
        }
    }
}

impl<A, B> AsyncWrite for EitherOutput<A, B>
where
    A: AsyncWrite + Unpin,
    B: AsyncWrite + Unpin,
{
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, IoError>> {
        match &mut *self {
            EitherOutput::First(a) => AsyncWrite::poll_write(Pin::new(a), cx, buf),
            EitherOutput::Second(b) => AsyncWrite::poll_write(Pin::new(b), cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), IoError>> {
        match &mut *self {
            EitherOutput::First(a) => AsyncWrite::poll_flush(Pin::new(a), cx),
            EitherOutput::Second(b) => AsyncWrite::poll_flush(Pin::new(b), cx),
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), IoError>> {
        match &mut *self {
            EitherOutput::First(a) => AsyncWrite::poll_close(Pin::new(a), cx),
            EitherOutput::Second(b) => AsyncWrite::poll_close(Pin::new(b), cx),
        }
    }
}

// TODO: remove?
impl<A, B> Write for EitherOutput<A, B>
where
    A: Write,
    B: Write,
{
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        match self {
            EitherOutput::First(a) => a.write(buf),
            EitherOutput::Second(b) => b.write(buf),
        }
    }

    fn flush(&mut self) -> Result<(), IoError> {
        match self {
            EitherOutput::First(a) => a.flush(),
            EitherOutput::Second(b) => b.flush(),
        }
    }
}

impl<A, B, I> Stream for EitherOutput<A, B>
where
    A: TryStream<Ok = I> + Unpin,
    B: TryStream<Ok = I> + Unpin,
{
    type Item = Result<I, EitherError<A::Error, B::Error>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match &mut *self {
            EitherOutput::First(a) => TryStream::try_poll_next(Pin::new(a), cx)
                .map(|v| v.map(|r| r.map_err(EitherError::A))),
            EitherOutput::Second(b) => TryStream::try_poll_next(Pin::new(b), cx)
                .map(|v| v.map(|r| r.map_err(EitherError::B))),
        }
    }
}

impl<A, B, I> Sink<I> for EitherOutput<A, B>
where
    A: Sink<I> + Unpin,
    B: Sink<I> + Unpin,
{
    type Error = EitherError<A::Error, B::Error>;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match &mut *self {
            EitherOutput::First(a) => Sink::poll_ready(Pin::new(a), cx).map_err(EitherError::A),
            EitherOutput::Second(b) => Sink::poll_ready(Pin::new(b), cx).map_err(EitherError::B),
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: I) -> Result<(), Self::Error> {
        match &mut *self {
            EitherOutput::First(a) => Sink::start_send(Pin::new(a), item).map_err(EitherError::A),
            EitherOutput::Second(b) => Sink::start_send(Pin::new(b), item).map_err(EitherError::B),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match &mut *self {
            EitherOutput::First(a) => Sink::poll_flush(Pin::new(a), cx).map_err(EitherError::A),
            EitherOutput::Second(b) => Sink::poll_flush(Pin::new(b), cx).map_err(EitherError::B),
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match &mut *self {
            EitherOutput::First(a) => Sink::poll_close(Pin::new(a), cx).map_err(EitherError::A),
            EitherOutput::Second(b) => Sink::poll_close(Pin::new(b), cx).map_err(EitherError::B),
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
    type Error = IoError;

    fn poll_inbound(&self, cx: &mut Context) -> Poll<Result<Self::Substream, Self::Error>> {
        match self {
            EitherOutput::First(inner) => inner.poll_inbound(cx).map(|p| p.map(EitherOutput::First)).map_err(|e| e.into()),
            EitherOutput::Second(inner) => inner.poll_inbound(cx).map(|p| p.map(EitherOutput::Second)).map_err(|e| e.into()),
        }
    }

    fn open_outbound(&self) -> Self::OutboundSubstream {
        match self {
            EitherOutput::First(inner) => EitherOutbound::A(inner.open_outbound()),
            EitherOutput::Second(inner) => EitherOutbound::B(inner.open_outbound()),
        }
    }

    fn poll_outbound(&self, cx: &mut Context, substream: &mut Self::OutboundSubstream) -> Poll<Result<Self::Substream, Self::Error>> {
        match (self, substream) {
            (EitherOutput::First(ref inner), EitherOutbound::A(ref mut substream)) => {
                inner.poll_outbound(cx, substream).map(|p| p.map(EitherOutput::First)).map_err(|e| e.into())
            },
            (EitherOutput::Second(ref inner), EitherOutbound::B(ref mut substream)) => {
                inner.poll_outbound(cx, substream).map(|p| p.map(EitherOutput::Second)).map_err(|e| e.into())
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

    fn read_substream(&self, cx: &mut Context, sub: &mut Self::Substream, buf: &mut [u8]) -> Poll<Result<usize, Self::Error>> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.read_substream(cx, sub, buf).map_err(|e| e.into())
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.read_substream(cx, sub, buf).map_err(|e| e.into())
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn write_substream(&self, cx: &mut Context, sub: &mut Self::Substream, buf: &[u8]) -> Poll<Result<usize, Self::Error>> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.write_substream(cx, sub, buf).map_err(|e| e.into())
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.write_substream(cx, sub, buf).map_err(|e| e.into())
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn flush_substream(&self, cx: &mut Context, sub: &mut Self::Substream) -> Poll<Result<(), Self::Error>> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.flush_substream(cx, sub).map_err(|e| e.into())
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.flush_substream(cx, sub).map_err(|e| e.into())
            },
            _ => panic!("Wrong API usage")
        }
    }

    fn shutdown_substream(&self, cx: &mut Context, sub: &mut Self::Substream) -> Poll<Result<(), Self::Error>> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.shutdown_substream(cx, sub).map_err(|e| e.into())
            },
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.shutdown_substream(cx, sub).map_err(|e| e.into())
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

    fn close(&self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match self {
            EitherOutput::First(inner) => inner.close(cx).map_err(|e| e.into()),
            EitherOutput::Second(inner) => inner.close(cx).map_err(|e| e.into()),
        }
    }

    fn flush_all(&self, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match self {
            EitherOutput::First(inner) => inner.flush_all(cx).map_err(|e| e.into()),
            EitherOutput::Second(inner) => inner.flush_all(cx).map_err(|e| e.into()),
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
    AStream: TryStream<Ok = ListenerEvent<AInner>> + Unpin,
    BStream: TryStream<Ok = ListenerEvent<BInner>> + Unpin,
{
    type Item = Result<ListenerEvent<EitherFuture<AInner, BInner>>, EitherError<AStream::Error, BStream::Error>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match &mut *self {
            EitherListenStream::First(a) => match TryStream::try_poll_next(Pin::new(a), cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Ready(Some(Ok(le))) => Poll::Ready(Some(Ok(le.map(EitherFuture::First)))),
                Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(EitherError::A(err)))),
            },
            EitherListenStream::Second(a) => match TryStream::try_poll_next(Pin::new(a), cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Ready(Some(Ok(le))) => Poll::Ready(Some(Ok(le.map(EitherFuture::Second)))),
                Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(EitherError::B(err)))),
            },
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
    AFuture: TryFuture<Ok = AInner> + Unpin,
    BFuture: TryFuture<Ok = BInner> + Unpin,
{
    type Output = Result<EitherOutput<AInner, BInner>, EitherError<AFuture::Error, BFuture::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match &mut *self {
            EitherFuture::First(a) => TryFuture::try_poll(Pin::new(a), cx)
                .map_ok(EitherOutput::First).map_err(EitherError::A),
            EitherFuture::Second(a) => TryFuture::try_poll(Pin::new(a), cx)
                .map_ok(EitherOutput::Second).map_err(EitherError::B),
        }
    }
}

#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherFuture2<A, B> { A(A), B(B) }

impl<AFut, BFut, AItem, BItem, AError, BError> Future for EitherFuture2<AFut, BFut>
where
    AFut: TryFuture<Ok = AItem, Error = AError> + Unpin,
    BFut: TryFuture<Ok = BItem, Error = BError> + Unpin,
{
    type Output = Result<EitherOutput<AItem, BItem>, EitherError<AError, BError>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match &mut *self {
            EitherFuture2::A(a) => TryFuture::try_poll(Pin::new(a), cx)
                .map_ok(EitherOutput::First).map_err(EitherError::A),
            EitherFuture2::B(a) => TryFuture::try_poll(Pin::new(a), cx)
                .map_ok(EitherOutput::Second).map_err(EitherError::B),
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
