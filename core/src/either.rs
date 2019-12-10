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
use pin_project::{pin_project, project};
use std::{fmt, io::{Error as IoError}, pin::Pin, task::Context, task::Poll};

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
#[pin_project]
#[derive(Debug, Copy, Clone)]
pub enum EitherOutput<A, B> {
    First(#[pin] A),
    Second(#[pin] B),
}

impl<A, B> AsyncRead for EitherOutput<A, B>
where
    A: AsyncRead,
    B: AsyncRead,
{
    #[project]
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context, buf: &mut [u8]) -> Poll<Result<usize, IoError>> {
        #[project]
        match self.project() {
            EitherOutput::First(a) => AsyncRead::poll_read(a, cx, buf),
            EitherOutput::Second(b) => AsyncRead::poll_read(b, cx, buf),
        }
    }
}

impl<A, B> AsyncWrite for EitherOutput<A, B>
where
    A: AsyncWrite,
    B: AsyncWrite,
{
    #[project]
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, IoError>> {
        #[project]
        match self.project() {
            EitherOutput::First(a) => AsyncWrite::poll_write(a, cx, buf),
            EitherOutput::Second(b) => AsyncWrite::poll_write(b, cx, buf),
        }
    }

    #[project]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), IoError>> {
        #[project]
        match self.project() {
            EitherOutput::First(a) => AsyncWrite::poll_flush(a, cx),
            EitherOutput::Second(b) => AsyncWrite::poll_flush(b, cx),
        }
    }

    #[project]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), IoError>> {
        #[project]
        match self.project() {
            EitherOutput::First(a) => AsyncWrite::poll_close(a, cx),
            EitherOutput::Second(b) => AsyncWrite::poll_close(b, cx),
        }
    }
}

impl<A, B, I> Stream for EitherOutput<A, B>
where
    A: TryStream<Ok = I>,
    B: TryStream<Ok = I>,
{
    type Item = Result<I, EitherError<A::Error, B::Error>>;

    #[project]
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        #[project]
        match self.project() {
            EitherOutput::First(a) => TryStream::try_poll_next(a, cx)
                .map(|v| v.map(|r| r.map_err(EitherError::A))),
            EitherOutput::Second(b) => TryStream::try_poll_next(b, cx)
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

    #[project]
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        #[project]
        match self.project() {
            EitherOutput::First(a) => Sink::poll_ready(a, cx).map_err(EitherError::A),
            EitherOutput::Second(b) => Sink::poll_ready(b, cx).map_err(EitherError::B),
        }
    }

    #[project]
    fn start_send(self: Pin<&mut Self>, item: I) -> Result<(), Self::Error> {
        #[project]
        match self.project() {
            EitherOutput::First(a) => Sink::start_send(a, item).map_err(EitherError::A),
            EitherOutput::Second(b) => Sink::start_send(b, item).map_err(EitherError::B),
        }
    }

    #[project]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        #[project]
        match self.project() {
            EitherOutput::First(a) => Sink::poll_flush(a, cx).map_err(EitherError::A),
            EitherOutput::Second(b) => Sink::poll_flush(b, cx).map_err(EitherError::B),
        }
    }

    #[project]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        #[project]
        match self.project() {
            EitherOutput::First(a) => Sink::poll_close(a, cx).map_err(EitherError::A),
            EitherOutput::Second(b) => Sink::poll_close(b, cx).map_err(EitherError::B),
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
#[pin_project]
#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherListenStream<A, B> {
    First(#[pin] A),
    Second(#[pin] B),
}

impl<AStream, BStream, AInner, BInner> Stream for EitherListenStream<AStream, BStream>
where
    AStream: TryStream<Ok = ListenerEvent<AInner>>,
    BStream: TryStream<Ok = ListenerEvent<BInner>>,
{
    type Item = Result<ListenerEvent<EitherFuture<AInner, BInner>>, EitherError<AStream::Error, BStream::Error>>;

    #[project]
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        #[project]
        match self.project() {
            EitherListenStream::First(a) => match TryStream::try_poll_next(a, cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Ready(Some(Ok(le))) => Poll::Ready(Some(Ok(le.map(EitherFuture::First)))),
                Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(EitherError::A(err)))),
            },
            EitherListenStream::Second(a) => match TryStream::try_poll_next(a, cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Ready(Some(Ok(le))) => Poll::Ready(Some(Ok(le.map(EitherFuture::Second)))),
                Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(EitherError::B(err)))),
            },
        }
    }
}

/// Implements `Future` and dispatches all method calls to either `First` or `Second`.
#[pin_project]
#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherFuture<A, B> {
    First(#[pin] A),
    Second(#[pin] B),
}

impl<AFuture, BFuture, AInner, BInner> Future for EitherFuture<AFuture, BFuture>
where
    AFuture: TryFuture<Ok = AInner>,
    BFuture: TryFuture<Ok = BInner>,
{
    type Output = Result<EitherOutput<AInner, BInner>, EitherError<AFuture::Error, BFuture::Error>>;

    #[project]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        #[project]
        match self.project() {
            EitherFuture::First(a) => TryFuture::try_poll(a, cx)
                .map_ok(EitherOutput::First).map_err(EitherError::A),
            EitherFuture::Second(a) => TryFuture::try_poll(a, cx)
                .map_ok(EitherOutput::Second).map_err(EitherError::B),
        }
    }
}

#[pin_project]
#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherFuture2<A, B> { A(#[pin] A), B(#[pin] B) }

impl<AFut, BFut, AItem, BItem, AError, BError> Future for EitherFuture2<AFut, BFut>
where
    AFut: TryFuture<Ok = AItem, Error = AError> + Unpin,
    BFut: TryFuture<Ok = BItem, Error = BError> + Unpin,
{
    type Output = Result<EitherOutput<AItem, BItem>, EitherError<AError, BError>>;

    #[project]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        #[project]
        match self.project() {
            EitherFuture2::A(a) => TryFuture::try_poll(a, cx)
                .map_ok(EitherOutput::First).map_err(EitherError::A),
            EitherFuture2::B(a) => TryFuture::try_poll(a, cx)
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
