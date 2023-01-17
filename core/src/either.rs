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

use crate::muxing::StreamMuxerEvent;
use crate::{
    muxing::StreamMuxer,
    transport::{ListenerId, Transport, TransportError, TransportEvent},
    Multiaddr, ProtocolName,
};
use either::Either;
use futures::{
    io::{IoSlice, IoSliceMut},
    prelude::*,
};
use pin_project::pin_project;
use std::{io, pin::Pin, task::Context, task::Poll};

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
    ) -> Poll<io::Result<usize>> {
        match self.project() {
            EitherOutputProj::First(a) => AsyncRead::poll_read(a, cx, buf),
            EitherOutputProj::Second(b) => AsyncRead::poll_read(b, cx, buf),
        }
    }

    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>> {
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
    ) -> Poll<io::Result<usize>> {
        match self.project() {
            EitherOutputProj::First(a) => AsyncWrite::poll_write(a, cx, buf),
            EitherOutputProj::Second(b) => AsyncWrite::poll_write(b, cx, buf),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        match self.project() {
            EitherOutputProj::First(a) => AsyncWrite::poll_write_vectored(a, cx, bufs),
            EitherOutputProj::Second(b) => AsyncWrite::poll_write_vectored(b, cx, bufs),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.project() {
            EitherOutputProj::First(a) => AsyncWrite::poll_flush(a, cx),
            EitherOutputProj::Second(b) => AsyncWrite::poll_flush(b, cx),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
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
    type Item = Result<I, Either<A::Error, B::Error>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.project() {
            EitherOutputProj::First(a) => {
                TryStream::try_poll_next(a, cx).map(|v| v.map(|r| r.map_err(Either::Left)))
            }
            EitherOutputProj::Second(b) => {
                TryStream::try_poll_next(b, cx).map(|v| v.map(|r| r.map_err(Either::Right)))
            }
        }
    }
}

impl<A, B, I> Sink<I> for EitherOutput<A, B>
where
    A: Sink<I>,
    B: Sink<I>,
{
    type Error = Either<A::Error, B::Error>;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            EitherOutputProj::First(a) => Sink::poll_ready(a, cx).map_err(Either::Left),
            EitherOutputProj::Second(b) => Sink::poll_ready(b, cx).map_err(Either::Right),
        }
    }

    fn start_send(self: Pin<&mut Self>, item: I) -> Result<(), Self::Error> {
        match self.project() {
            EitherOutputProj::First(a) => Sink::start_send(a, item).map_err(Either::Left),
            EitherOutputProj::Second(b) => Sink::start_send(b, item).map_err(Either::Right),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            EitherOutputProj::First(a) => Sink::poll_flush(a, cx).map_err(Either::Left),
            EitherOutputProj::Second(b) => Sink::poll_flush(b, cx).map_err(Either::Right),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            EitherOutputProj::First(a) => Sink::poll_close(a, cx).map_err(Either::Left),
            EitherOutputProj::Second(b) => Sink::poll_close(b, cx).map_err(Either::Right),
        }
    }
}

impl<A, B> StreamMuxer for EitherOutput<A, B>
where
    A: StreamMuxer,
    B: StreamMuxer,
{
    type Substream = EitherOutput<A::Substream, B::Substream>;
    type Error = Either<A::Error, B::Error>;

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        match self.project() {
            EitherOutputProj::First(inner) => inner
                .poll_inbound(cx)
                .map_ok(EitherOutput::First)
                .map_err(Either::Left),
            EitherOutputProj::Second(inner) => inner
                .poll_inbound(cx)
                .map_ok(EitherOutput::Second)
                .map_err(Either::Right),
        }
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        match self.project() {
            EitherOutputProj::First(inner) => inner
                .poll_outbound(cx)
                .map_ok(EitherOutput::First)
                .map_err(Either::Left),
            EitherOutputProj::Second(inner) => inner
                .poll_outbound(cx)
                .map_ok(EitherOutput::Second)
                .map_err(Either::Right),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.project() {
            EitherOutputProj::First(inner) => inner.poll_close(cx).map_err(Either::Left),
            EitherOutputProj::Second(inner) => inner.poll_close(cx).map_err(Either::Right),
        }
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        match self.project() {
            EitherOutputProj::First(inner) => inner.poll(cx).map_err(Either::Left),
            EitherOutputProj::Second(inner) => inner.poll(cx).map_err(Either::Right),
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
    type Output = Result<EitherOutput<AInner, BInner>, Either<AFuture::Error, BFuture::Error>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EitherFutureProj::First(a) => TryFuture::try_poll(a, cx)
                .map_ok(EitherOutput::First)
                .map_err(Either::Left),
            EitherFutureProj::Second(a) => TryFuture::try_poll(a, cx)
                .map_ok(EitherOutput::Second)
                .map_err(Either::Right),
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
    type Output = Result<EitherOutput<AItem, BItem>, Either<AError, BError>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EitherFuture2Proj::A(a) => TryFuture::try_poll(a, cx)
                .map_ok(EitherOutput::First)
                .map_err(Either::Left),
            EitherFuture2Proj::B(a) => TryFuture::try_poll(a, cx)
                .map_ok(EitherOutput::Second)
                .map_err(Either::Right),
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
#[pin_project(project = EitherTransportProj)]
#[derive(Debug)]
#[must_use = "transports do nothing unless polled"]
pub enum EitherTransport<A, B> {
    Left(#[pin] A),
    Right(#[pin] B),
}

impl<A, B> Transport for EitherTransport<A, B>
where
    B: Transport,
    A: Transport,
{
    type Output = EitherOutput<A::Output, B::Output>;
    type Error = Either<A::Error, B::Error>;
    type ListenerUpgrade = EitherFuture<A::ListenerUpgrade, B::ListenerUpgrade>;
    type Dial = EitherFuture<A::Dial, B::Dial>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        match self.project() {
            EitherTransportProj::Left(a) => match a.poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(event) => {
                    Poll::Ready(event.map_upgrade(EitherFuture::First).map_err(Either::Left))
                }
            },
            EitherTransportProj::Right(b) => match b.poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(event) => Poll::Ready(
                    event
                        .map_upgrade(EitherFuture::Second)
                        .map_err(Either::Right),
                ),
            },
        }
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        match self {
            EitherTransport::Left(t) => t.remove_listener(id),
            EitherTransport::Right(t) => t.remove_listener(id),
        }
    }

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        use TransportError::*;
        match self {
            EitherTransport::Left(a) => a.listen_on(addr).map_err(|e| match e {
                MultiaddrNotSupported(addr) => MultiaddrNotSupported(addr),
                Other(err) => Other(Either::Left(err)),
            }),
            EitherTransport::Right(b) => b.listen_on(addr).map_err(|e| match e {
                MultiaddrNotSupported(addr) => MultiaddrNotSupported(addr),
                Other(err) => Other(Either::Right(err)),
            }),
        }
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        use TransportError::*;
        match self {
            EitherTransport::Left(a) => match a.dial(addr) {
                Ok(connec) => Ok(EitherFuture::First(connec)),
                Err(MultiaddrNotSupported(addr)) => Err(MultiaddrNotSupported(addr)),
                Err(Other(err)) => Err(Other(Either::Left(err))),
            },
            EitherTransport::Right(b) => match b.dial(addr) {
                Ok(connec) => Ok(EitherFuture::Second(connec)),
                Err(MultiaddrNotSupported(addr)) => Err(MultiaddrNotSupported(addr)),
                Err(Other(err)) => Err(Other(Either::Right(err))),
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
                Err(Other(err)) => Err(Other(Either::Left(err))),
            },
            EitherTransport::Right(b) => match b.dial_as_listener(addr) {
                Ok(connec) => Ok(EitherFuture::Second(connec)),
                Err(MultiaddrNotSupported(addr)) => Err(MultiaddrNotSupported(addr)),
                Err(Other(err)) => Err(Other(Either::Right(err))),
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
