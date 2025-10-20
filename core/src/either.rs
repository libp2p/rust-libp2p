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

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use either::Either;
use futures::prelude::*;
use pin_project::pin_project;

use crate::{
    muxing::{StreamMuxer, StreamMuxerEvent},
    transport::{DialOpts, ListenerId, PortUse, Transport, TransportError, TransportEvent},
    Multiaddr,
};

impl<A, B> StreamMuxer for future::Either<A, B>
where
    A: StreamMuxer,
    B: StreamMuxer,
{
    type Substream = future::Either<A::Substream, B::Substream>;
    type Error = Either<A::Error, B::Error>;

    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        match self.as_pin_mut() {
            future::Either::Left(inner) => inner
                .poll_inbound(cx)
                .map_ok(future::Either::Left)
                .map_err(Either::Left),
            future::Either::Right(inner) => inner
                .poll_inbound(cx)
                .map_ok(future::Either::Right)
                .map_err(Either::Right),
        }
    }

    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        match self.as_pin_mut() {
            future::Either::Left(inner) => inner
                .poll_outbound(cx)
                .map_ok(future::Either::Left)
                .map_err(Either::Left),
            future::Either::Right(inner) => inner
                .poll_outbound(cx)
                .map_ok(future::Either::Right)
                .map_err(Either::Right),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.as_pin_mut() {
            future::Either::Left(inner) => inner.poll_close(cx).map_err(Either::Left),
            future::Either::Right(inner) => inner.poll_close(cx).map_err(Either::Right),
        }
    }

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        match self.as_pin_mut() {
            future::Either::Left(inner) => inner.poll(cx).map_err(Either::Left),
            future::Either::Right(inner) => inner.poll(cx).map_err(Either::Right),
        }
    }
}

/// Future for listener upgrades that returns Either<A::Output, B::Output>
#[pin_project(project = EitherUpgradeFutureProj)]
#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherUpgradeFuture<A, B> {
    First(#[pin] A),
    Second(#[pin] B),
}

impl<AFuture, BFuture, AInner, BInner> Future for EitherUpgradeFuture<AFuture, BFuture>
where
    AFuture: TryFuture<Ok = AInner>,
    BFuture: TryFuture<Ok = BInner>,
{
    type Output = Result<future::Either<AInner, BInner>, Either<AFuture::Error, BFuture::Error>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EitherUpgradeFutureProj::First(a) => TryFuture::try_poll(a, cx)
                .map_ok(future::Either::Left)
                .map_err(Either::Left),
            EitherUpgradeFutureProj::Second(b) => TryFuture::try_poll(b, cx)
                .map_ok(future::Either::Right)
                .map_err(Either::Right),
        }
    }
}

/// Future for dials that returns (Either<A::Output, B::Output>, PortUse)
#[pin_project(project = EitherDialFutureProj)]
#[derive(Debug, Copy, Clone)]
#[must_use = "futures do nothing unless polled"]
pub enum EitherDialFuture<A, B> {
    First(#[pin] A),
    Second(#[pin] B),
}

impl<AFuture, BFuture, AInner, BInner> Future for EitherDialFuture<AFuture, BFuture>
where
    AFuture: TryFuture<Ok = (AInner, PortUse)>,
    BFuture: TryFuture<Ok = (BInner, PortUse)>,
{
    type Output =
        Result<(future::Either<AInner, BInner>, PortUse), Either<AFuture::Error, BFuture::Error>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EitherDialFutureProj::First(a) => TryFuture::try_poll(a, cx)
                .map_ok(|(inner, port_use)| (future::Either::Left(inner), port_use))
                .map_err(Either::Left),
            EitherDialFutureProj::Second(b) => TryFuture::try_poll(b, cx)
                .map_ok(|(inner, port_use)| (future::Either::Right(inner), port_use))
                .map_err(Either::Right),
        }
    }
}

impl<A, B> Transport for Either<A, B>
where
    B: Transport,
    A: Transport,
{
    type Output = future::Either<A::Output, B::Output>;
    type Error = Either<A::Error, B::Error>;
    type ListenerUpgrade = EitherUpgradeFuture<A::ListenerUpgrade, B::ListenerUpgrade>;
    type Dial = EitherDialFuture<A::Dial, B::Dial>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        match self.as_pin_mut() {
            Either::Left(a) => match a.poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(event) => Poll::Ready(
                    event
                        .map_upgrade(EitherUpgradeFuture::First)
                        .map_err(Either::Left),
                ),
            },
            Either::Right(b) => match b.poll(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(event) => Poll::Ready(
                    event
                        .map_upgrade(EitherUpgradeFuture::Second)
                        .map_err(Either::Right),
                ),
            },
        }
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        match self {
            Either::Left(t) => t.remove_listener(id),
            Either::Right(t) => t.remove_listener(id),
        }
    }

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        use TransportError::*;
        match self {
            Either::Left(a) => a.listen_on(id, addr).map_err(|e| match e {
                MultiaddrNotSupported(addr) => MultiaddrNotSupported(addr),
                Other(err) => Other(Either::Left(err)),
            }),
            Either::Right(b) => b.listen_on(id, addr).map_err(|e| match e {
                MultiaddrNotSupported(addr) => MultiaddrNotSupported(addr),
                Other(err) => Other(Either::Right(err)),
            }),
        }
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        use TransportError::*;
        match self {
            Either::Left(a) => match a.dial(addr, opts) {
                Ok(connec) => Ok(EitherDialFuture::First(connec)),
                Err(MultiaddrNotSupported(addr)) => Err(MultiaddrNotSupported(addr)),
                Err(Other(err)) => Err(Other(Either::Left(err))),
            },
            Either::Right(b) => match b.dial(addr, opts) {
                Ok(connec) => Ok(EitherDialFuture::Second(connec)),
                Err(MultiaddrNotSupported(addr)) => Err(MultiaddrNotSupported(addr)),
                Err(Other(err)) => Err(Other(Either::Right(err))),
            },
        }
    }
}
