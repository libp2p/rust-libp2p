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

use crate::{
    muxing::{StreamMuxer, StreamMuxerEvent},
    transport::{ListenerEvent, Transport, TransportError},
    Multiaddr, ProtocolName,
};
use futures::{
    io::{IoSlice, IoSliceMut},
    prelude::*,
};
use pin_project::pin_project;
use std::{fmt, io::Error as IoError, pin::Pin, task::Context, task::Poll};

#[derive(Debug, Copy, Clone)]
pub enum EitherError<A, B> {
    A(A),
    B(B),
}

impl<A, B> fmt::Display for EitherError<A, B>
where
    A: fmt::Display,
    B: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EitherError::A(a) => a.fmt(f),
            EitherError::B(b) => b.fmt(f),
        }
    }
}

impl<A, B> std::error::Error for EitherError<A, B>
where
    A: std::error::Error,
    B: std::error::Error,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EitherError::A(a) => a.source(),
            EitherError::B(b) => b.source(),
        }
    }
}

/// Implements `AsyncRead` and `AsyncWrite` and dispatches all method calls to
/// either `First` or `Second`.
#[pin_project(project = EitherOutputProj)]
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
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, IoError>> {
        match self.project() {
            EitherOutputProj::First(a) => AsyncRead::poll_read(a, cx, buf),
            EitherOutputProj::Second(b) => AsyncRead::poll_read(b, cx, buf),
        }
    }

    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<Result<usize, IoError>> {
        match self.project() {
            EitherOutputProj::First(a) => AsyncRead::poll_read_vectored(a, cx, bufs),
            EitherOutputProj::Second(b) => AsyncRead::poll_read_vectored(b, cx, bufs),
        }
    }
}

impl<A, B> AsyncWrite for EitherOutput<A, B>
where
    A: AsyncWrite,
    B: AsyncWrite,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, IoError>> {
        match self.project() {
            EitherOutputProj::First(a) => AsyncWrite::poll_write(a, cx, buf),
            EitherOutputProj::Second(b) => AsyncWrite::poll_write(b, cx, buf),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<Result<usize, IoError>> {
        match self.project() {
            EitherOutputProj::First(a) => AsyncWrite::poll_write_vectored(a, cx, bufs),
            EitherOutputProj::Second(b) => AsyncWrite::poll_write_vectored(b, cx, bufs),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        match self.project() {
            EitherOutputProj::First(a) => AsyncWrite::poll_flush(a, cx),
            EitherOutputProj::Second(b) => AsyncWrite::poll_flush(b, cx),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), IoError>> {
        match self.project() {
            EitherOutputProj::First(a) => AsyncWrite::poll_close(a, cx),
            EitherOutputProj::Second(b) => AsyncWrite::poll_close(b, cx),
        }
    }
}

impl<A, B, I> Stream for EitherOutput<A, B>
where
    A: TryStream<Ok = I>,
    B: TryStream<Ok = I>,
{
    type Item = Result<I, EitherError<A::Error, B::Error>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.project() {
            EitherOutputProj::First(a) => {
                TryStream::try_poll_next(a, cx).map(|v| v.map(|r| r.map_err(EitherError::A)))
            }
            EitherOutputProj::Second(b) => {
                TryStream::try_poll_next(b, cx).map(|v| v.map(|r| r.map_err(EitherError::B)))
            }
        }
    }
}

impl<A, B, I> Sink<I> for EitherOutput<A, B>
where
    A: Sink<I>,
    B: Sink<I>,
{
    type Error = EitherError<A::Error, B::Error>;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            EitherOutputProj::First(a) => Sink::poll_ready(a, cx).map_err(EitherError::A),
            EitherOutputProj::Second(b) => Sink::poll_ready(b, cx).map_err(EitherError::B),
        }
    }

    fn start_send(self: Pin<&mut Self>, item: I) -> Result<(), Self::Error> {
        match self.project() {
            EitherOutputProj::First(a) => Sink::start_send(a, item).map_err(EitherError::A),
            EitherOutputProj::Second(b) => Sink::start_send(b, item).map_err(EitherError::B),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            EitherOutputProj::First(a) => Sink::poll_flush(a, cx).map_err(EitherError::A),
            EitherOutputProj::Second(b) => Sink::poll_flush(b, cx).map_err(EitherError::B),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            EitherOutputProj::First(a) => Sink::poll_close(a, cx).map_err(EitherError::A),
            EitherOutputProj::Second(b) => Sink::poll_close(b, cx).map_err(EitherError::B),
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

    fn poll_event(
        &self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent<Self::Substream>, Self::Error>> {
        match self {
            EitherOutput::First(inner) => inner.poll_event(cx).map(|result| {
                result.map_err(|e| e.into()).map(|event| match event {
                    StreamMuxerEvent::AddressChange(addr) => StreamMuxerEvent::AddressChange(addr),
                    StreamMuxerEvent::InboundSubstream(substream) => {
                        StreamMuxerEvent::InboundSubstream(EitherOutput::First(substream))
                    }
                })
            }),
            EitherOutput::Second(inner) => inner.poll_event(cx).map(|result| {
                result.map_err(|e| e.into()).map(|event| match event {
                    StreamMuxerEvent::AddressChange(addr) => StreamMuxerEvent::AddressChange(addr),
                    StreamMuxerEvent::InboundSubstream(substream) => {
                        StreamMuxerEvent::InboundSubstream(EitherOutput::Second(substream))
                    }
                })
            }),
        }
    }

    fn open_outbound(&self) -> Self::OutboundSubstream {
        match self {
            EitherOutput::First(inner) => EitherOutbound::A(inner.open_outbound()),
            EitherOutput::Second(inner) => EitherOutbound::B(inner.open_outbound()),
        }
    }

    fn poll_outbound(
        &self,
        cx: &mut Context<'_>,
        substream: &mut Self::OutboundSubstream,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        match (self, substream) {
            (EitherOutput::First(ref inner), EitherOutbound::A(ref mut substream)) => inner
                .poll_outbound(cx, substream)
                .map(|p| p.map(EitherOutput::First))
                .map_err(|e| e.into()),
            (EitherOutput::Second(ref inner), EitherOutbound::B(ref mut substream)) => inner
                .poll_outbound(cx, substream)
                .map(|p| p.map(EitherOutput::Second))
                .map_err(|e| e.into()),
            _ => panic!("Wrong API usage"),
        }
    }

    fn destroy_outbound(&self, substream: Self::OutboundSubstream) {
        match self {
            EitherOutput::First(inner) => match substream {
                EitherOutbound::A(substream) => inner.destroy_outbound(substream),
                _ => panic!("Wrong API usage"),
            },
            EitherOutput::Second(inner) => match substream {
                EitherOutbound::B(substream) => inner.destroy_outbound(substream),
                _ => panic!("Wrong API usage"),
            },
        }
    }

    fn read_substream(
        &self,
        cx: &mut Context<'_>,
        sub: &mut Self::Substream,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Self::Error>> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.read_substream(cx, sub, buf).map_err(|e| e.into())
            }
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.read_substream(cx, sub, buf).map_err(|e| e.into())
            }
            _ => panic!("Wrong API usage"),
        }
    }

    fn write_substream(
        &self,
        cx: &mut Context<'_>,
        sub: &mut Self::Substream,
        buf: &[u8],
    ) -> Poll<Result<usize, Self::Error>> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.write_substream(cx, sub, buf).map_err(|e| e.into())
            }
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.write_substream(cx, sub, buf).map_err(|e| e.into())
            }
            _ => panic!("Wrong API usage"),
        }
    }

    fn flush_substream(
        &self,
        cx: &mut Context<'_>,
        sub: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.flush_substream(cx, sub).map_err(|e| e.into())
            }
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.flush_substream(cx, sub).map_err(|e| e.into())
            }
            _ => panic!("Wrong API usage"),
        }
    }

    fn shutdown_substream(
        &self,
        cx: &mut Context<'_>,
        sub: &mut Self::Substream,
    ) -> Poll<Result<(), Self::Error>> {
        match (self, sub) {
            (EitherOutput::First(ref inner), EitherOutput::First(ref mut sub)) => {
                inner.shutdown_substream(cx, sub).map_err(|e| e.into())
            }
            (EitherOutput::Second(ref inner), EitherOutput::Second(ref mut sub)) => {
                inner.shutdown_substream(cx, sub).map_err(|e| e.into())
            }
            _ => panic!("Wrong API usage"),
        }
    }

    fn destroy_substream(&self, substream: Self::Substream) {
        match self {
            EitherOutput::First(inner) => match substream {
                EitherOutput::First(substream) => inner.destroy_substream(substream),
                _ => panic!("Wrong API usage"),
            },
            EitherOutput::Second(inner) => match substream {
                EitherOutput::Second(substream) => inner.destroy_substream(substream),
                _ => panic!("Wrong API usage"),
            },
        }
    }

    fn poll_close(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self {
            EitherOutput::First(inner) => inner.poll_close(cx).map_err(|e| e.into()),
            EitherOutput::Second(inner) => inner.poll_close(cx).map_err(|e| e.into()),
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
#[pin_project(project = EitherListenStreamProj)]
#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherListenStream<A, B> {
    First(#[pin] A),
    Second(#[pin] B),
}

impl<AStream, BStream, AInner, BInner, AError, BError> Stream
    for EitherListenStream<AStream, BStream>
where
    AStream: TryStream<Ok = ListenerEvent<AInner, AError>, Error = AError>,
    BStream: TryStream<Ok = ListenerEvent<BInner, BError>, Error = BError>,
{
    type Item = Result<
        ListenerEvent<EitherFuture<AInner, BInner>, EitherError<AError, BError>>,
        EitherError<AError, BError>,
    >;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.project() {
            EitherListenStreamProj::First(a) => match TryStream::try_poll_next(a, cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Ready(Some(Ok(le))) => Poll::Ready(Some(Ok(le
                    .map(EitherFuture::First)
                    .map_err(EitherError::A)))),
                Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(EitherError::A(err)))),
            },
            EitherListenStreamProj::Second(a) => match TryStream::try_poll_next(a, cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Ready(Some(Ok(le))) => Poll::Ready(Some(Ok(le
                    .map(EitherFuture::Second)
                    .map_err(EitherError::B)))),
                Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(EitherError::B(err)))),
            },
        }
    }
}

/// Implements `Future` and dispatches all method calls to either `First` or `Second`.
#[pin_project(project = EitherFutureProj)]
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

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EitherFutureProj::First(a) => TryFuture::try_poll(a, cx)
                .map_ok(EitherOutput::First)
                .map_err(EitherError::A),
            EitherFutureProj::Second(a) => TryFuture::try_poll(a, cx)
                .map_ok(EitherOutput::Second)
                .map_err(EitherError::B),
        }
    }
}

#[pin_project(project = EitherFuture2Proj)]
#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherFuture2<A, B> {
    A(#[pin] A),
    B(#[pin] B),
}

impl<AFut, BFut, AItem, BItem, AError, BError> Future for EitherFuture2<AFut, BFut>
where
    AFut: TryFuture<Ok = AItem, Error = AError>,
    BFut: TryFuture<Ok = BItem, Error = BError>,
{
    type Output = Result<EitherOutput<AItem, BItem>, EitherError<AError, BError>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EitherFuture2Proj::A(a) => TryFuture::try_poll(a, cx)
                .map_ok(EitherOutput::First)
                .map_err(EitherError::A),
            EitherFuture2Proj::B(a) => TryFuture::try_poll(a, cx)
                .map_ok(EitherOutput::Second)
                .map_err(EitherError::B),
        }
    }
}

#[derive(Debug, Clone)]
pub enum EitherName<A, B> {
    A(A),
    B(B),
}

impl<A: ProtocolName, B: ProtocolName> ProtocolName for EitherName<A, B> {
    fn protocol_name(&self) -> &[u8] {
        match self {
            EitherName::A(a) => a.protocol_name(),
            EitherName::B(b) => b.protocol_name(),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum EitherTransport<A, B> {
    Left(A),
    Right(B),
}

impl<A, B> Transport for EitherTransport<A, B>
where
    B: Transport,
    A: Transport,
{
    type Output = EitherOutput<A::Output, B::Output>;
    type Error = EitherError<A::Error, B::Error>;
    type Listener = EitherListenStream<A::Listener, B::Listener>;
    type ListenerUpgrade = EitherFuture<A::ListenerUpgrade, B::ListenerUpgrade>;
    type Dial = EitherFuture<A::Dial, B::Dial>;

    fn listen_on(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Listener, TransportError<Self::Error>> {
        use TransportError::*;
        match self {
            EitherTransport::Left(a) => match a.listen_on(addr) {
                Ok(listener) => Ok(EitherListenStream::First(listener)),
                Err(MultiaddrNotSupported(addr)) => Err(MultiaddrNotSupported(addr)),
                Err(Other(err)) => Err(Other(EitherError::A(err))),
            },
            EitherTransport::Right(b) => match b.listen_on(addr) {
                Ok(listener) => Ok(EitherListenStream::Second(listener)),
                Err(MultiaddrNotSupported(addr)) => Err(MultiaddrNotSupported(addr)),
                Err(Other(err)) => Err(Other(EitherError::B(err))),
            },
        }
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        use TransportError::*;
        match self {
            EitherTransport::Left(a) => match a.dial(addr) {
                Ok(connec) => Ok(EitherFuture::First(connec)),
                Err(MultiaddrNotSupported(addr)) => Err(MultiaddrNotSupported(addr)),
                Err(Other(err)) => Err(Other(EitherError::A(err))),
            },
            EitherTransport::Right(b) => match b.dial(addr) {
                Ok(connec) => Ok(EitherFuture::Second(connec)),
                Err(MultiaddrNotSupported(addr)) => Err(MultiaddrNotSupported(addr)),
                Err(Other(err)) => Err(Other(EitherError::B(err))),
            },
        }
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>>
    where
        Self: Sized,
    {
        use TransportError::*;
        match self {
            EitherTransport::Left(a) => match a.dial_as_listener(addr) {
                Ok(connec) => Ok(EitherFuture::First(connec)),
                Err(MultiaddrNotSupported(addr)) => Err(MultiaddrNotSupported(addr)),
                Err(Other(err)) => Err(Other(EitherError::A(err))),
            },
            EitherTransport::Right(b) => match b.dial_as_listener(addr) {
                Ok(connec) => Ok(EitherFuture::Second(connec)),
                Err(MultiaddrNotSupported(addr)) => Err(MultiaddrNotSupported(addr)),
                Err(Other(err)) => Err(Other(EitherError::B(err))),
            },
        }
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        match self {
            EitherTransport::Left(a) => a.address_translation(server, observed),
            EitherTransport::Right(b) => b.address_translation(server, observed),
        }
    }
}
