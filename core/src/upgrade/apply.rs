// Copyright 2018 Parity Technologies (UK) Ltd.
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

use crate::{ConnectedPoint, Negotiated};
use crate::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeError, ProtocolName};
use futures::{future::Either, prelude::*, compat::Compat, compat::Compat01As03, compat::Future01CompatExt};
use log::debug;
use multistream_select::{self, DialerSelectFuture, ListenerSelectFuture};
use std::{iter, mem, pin::Pin, task::Context, task::Poll};

pub use multistream_select::Version;

/// Applies an upgrade to the inbound and outbound direction of a connection or substream.
pub fn apply<C, U>(conn: C, up: U, cp: ConnectedPoint, v: Version)
    -> Either<InboundUpgradeApply<C, U>, OutboundUpgradeApply<C, U>>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>> + OutboundUpgrade<Negotiated<C>>,
{
    if cp.is_listener() {
        Either::Left(apply_inbound(conn, up))
    } else {
        Either::Right(apply_outbound(conn, up, v))
    }
}

/// Tries to perform an upgrade on an inbound connection or substream.
pub fn apply_inbound<C, U>(conn: C, up: U) -> InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>>,
{
    let iter = up.protocol_info().into_iter().map(NameWrap as fn(_) -> NameWrap<_>);
    let future = multistream_select::listener_select_proto(Compat::new(conn), iter).compat();
    InboundUpgradeApply {
        inner: InboundUpgradeApplyState::Init { future, upgrade: up }
    }
}

/// Tries to perform an upgrade on an outbound connection or substream.
pub fn apply_outbound<C, U>(conn: C, up: U, v: Version) -> OutboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundUpgrade<Negotiated<C>>
{
    let iter = up.protocol_info().into_iter().map(NameWrap as fn(_) -> NameWrap<_>);
    let future = multistream_select::dialer_select_proto(Compat::new(conn), iter, v).compat();
    OutboundUpgradeApply {
        inner: OutboundUpgradeApplyState::Init { future, upgrade: up }
    }
}

/// Future returned by `apply_inbound`. Drives the upgrade process.
pub struct InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>>
{
    inner: InboundUpgradeApplyState<C, U>
}

enum InboundUpgradeApplyState<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>>,
{
    Init {
        future: Compat01As03<ListenerSelectFuture<Compat<C>, NameWrap<U::Info>>>,
        upgrade: U,
    },
    Upgrade {
        future: Pin<Box<U::Future>>
    },
    Undefined
}

impl<C, U> Unpin for InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>>,
{
}

impl<C, U> Future for InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>>,
{
    type Output = Result<U::Output, UpgradeError<U::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        loop {
            match mem::replace(&mut self.inner, InboundUpgradeApplyState::Undefined) {
                InboundUpgradeApplyState::Init { mut future, upgrade } => {
                    let (info, io) = match Future::poll(Pin::new(&mut future), cx)? {
                        Poll::Ready(x) => x,
                        Poll::Pending => {
                            self.inner = InboundUpgradeApplyState::Init { future, upgrade };
                            return Poll::Pending
                        }
                    };
                    self.inner = InboundUpgradeApplyState::Upgrade {
                        future: Box::pin(upgrade.upgrade_inbound(Compat01As03::new(io), info.0))
                    };
                }
                InboundUpgradeApplyState::Upgrade { mut future } => {
                    match Future::poll(Pin::new(&mut future), cx) {
                        Poll::Pending => {
                            self.inner = InboundUpgradeApplyState::Upgrade { future };
                            return Poll::Pending
                        }
                        Poll::Ready(Ok(x)) => {
                            debug!("Successfully applied negotiated protocol");
                            return Poll::Ready(Ok(x))
                        }
                        Poll::Ready(Err(e)) => {
                            debug!("Failed to apply negotiated protocol");
                            return Poll::Ready(Err(UpgradeError::Apply(e)))
                        }
                    }
                }
                InboundUpgradeApplyState::Undefined =>
                    panic!("InboundUpgradeApplyState::poll called after completion")
            }
        }
    }
}

/// Future returned by `apply_outbound`. Drives the upgrade process.
pub struct OutboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundUpgrade<Negotiated<C>>
{
    inner: OutboundUpgradeApplyState<C, U>
}

enum OutboundUpgradeApplyState<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundUpgrade<Negotiated<C>>
{
    Init {
        future: Compat01As03<DialerSelectFuture<Compat<C>, NameWrapIter<<U::InfoIter as IntoIterator>::IntoIter>>>,
        upgrade: U
    },
    Upgrade {
        future: Pin<Box<U::Future>>
    },
    Undefined
}

impl<C, U> Unpin for OutboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundUpgrade<Negotiated<C>>,
{
}

impl<C, U> Future for OutboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundUpgrade<Negotiated<C>>,
{
    type Output = Result<U::Output, UpgradeError<U::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        loop {
            match mem::replace(&mut self.inner, OutboundUpgradeApplyState::Undefined) {
                OutboundUpgradeApplyState::Init { mut future, upgrade } => {
                    let (info, connection) = match Future::poll(Pin::new(&mut future), cx)? {
                        Poll::Ready(x) => x,
                        Poll::Pending => {
                            self.inner = OutboundUpgradeApplyState::Init { future, upgrade };
                            return Poll::Pending
                        }
                    };
                    self.inner = OutboundUpgradeApplyState::Upgrade {
                        future: Box::pin(upgrade.upgrade_outbound(Compat01As03::new(connection), info.0))
                    };
                }
                OutboundUpgradeApplyState::Upgrade { mut future } => {
                    match Future::poll(Pin::new(&mut future), cx) {
                        Poll::Pending => {
                            self.inner = OutboundUpgradeApplyState::Upgrade { future };
                            return Poll::Pending
                        }
                        Poll::Ready(Ok(x)) => {
                            debug!("Successfully applied negotiated protocol");
                            return Poll::Ready(Ok(x))
                        }
                        Poll::Ready(Err(e)) => {
                            debug!("Failed to apply negotiated protocol");
                            return Poll::Ready(Err(UpgradeError::Apply(e)));
                        }
                    }
                }
                OutboundUpgradeApplyState::Undefined =>
                    panic!("OutboundUpgradeApplyState::poll called after completion")
            }
        }
    }
}

type NameWrapIter<I> = iter::Map<I, fn(<I as Iterator>::Item) -> NameWrap<<I as Iterator>::Item>>;

/// Wrapper type to expose an `AsRef<[u8]>` impl for all types implementing `ProtocolName`.
#[derive(Clone)]
struct NameWrap<N>(N);

impl<N: ProtocolName> AsRef<[u8]> for NameWrap<N> {
    fn as_ref(&self) -> &[u8] {
        self.0.protocol_name()
    }
}

