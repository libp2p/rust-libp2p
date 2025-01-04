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

use std::{
    mem,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{future::Either, prelude::*};
pub(crate) use multistream_select::Version;
use multistream_select::{DialerSelectFuture, ListenerSelectFuture};

use crate::{
    connection::ConnectedPoint,
    upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade, UpgradeError},
    Negotiated,
};

// TODO: Still needed?
/// Applies an upgrade to the inbound and outbound direction of a connection or substream.
pub(crate) fn apply<C, U>(
    conn: C,
    up: U,
    cp: ConnectedPoint,
    v: Version,
) -> Either<InboundUpgradeApply<C, U>, OutboundUpgradeApply<C, U>>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundConnectionUpgrade<Negotiated<C>> + OutboundConnectionUpgrade<Negotiated<C>>,
{
    match cp {
        ConnectedPoint::Dialer { role_override, .. } if role_override.is_dialer() => {
            Either::Right(apply_outbound(conn, up, v))
        }
        _ => Either::Left(apply_inbound(conn, up)),
    }
}

/// Tries to perform an upgrade on an inbound connection or substream.
pub(crate) fn apply_inbound<C, U>(conn: C, up: U) -> InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundConnectionUpgrade<Negotiated<C>>,
{
    InboundUpgradeApply {
        inner: InboundUpgradeApplyState::Init {
            future: multistream_select::listener_select_proto(conn, up.protocol_info()),
            upgrade: up,
        },
    }
}

/// Tries to perform an upgrade on an outbound connection or substream.
pub(crate) fn apply_outbound<C, U>(conn: C, up: U, v: Version) -> OutboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundConnectionUpgrade<Negotiated<C>>,
{
    OutboundUpgradeApply {
        inner: OutboundUpgradeApplyState::Init {
            future: multistream_select::dialer_select_proto(conn, up.protocol_info(), v),
            upgrade: up,
        },
    }
}

/// Future returned by `apply_inbound`. Drives the upgrade process.
pub struct InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundConnectionUpgrade<Negotiated<C>>,
{
    inner: InboundUpgradeApplyState<C, U>,
}

#[allow(clippy::large_enum_variant)]
enum InboundUpgradeApplyState<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundConnectionUpgrade<Negotiated<C>>,
{
    Init {
        future: ListenerSelectFuture<C, U::Info>,
        upgrade: U,
    },
    Upgrade {
        future: Pin<Box<U::Future>>,
        name: String,
    },
    Undefined,
}

impl<C, U> Unpin for InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundConnectionUpgrade<Negotiated<C>>,
{
}

impl<C, U> Future for InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundConnectionUpgrade<Negotiated<C>>,
{
    type Output = Result<U::Output, UpgradeError<U::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match mem::replace(&mut self.inner, InboundUpgradeApplyState::Undefined) {
                InboundUpgradeApplyState::Init {
                    mut future,
                    upgrade,
                } => {
                    let (info, io) = match Future::poll(Pin::new(&mut future), cx)? {
                        Poll::Ready(x) => x,
                        Poll::Pending => {
                            self.inner = InboundUpgradeApplyState::Init { future, upgrade };
                            return Poll::Pending;
                        }
                    };
                    self.inner = InboundUpgradeApplyState::Upgrade {
                        future: Box::pin(upgrade.upgrade_inbound(io, info.clone())),
                        name: info.as_ref().to_owned(),
                    };
                }
                InboundUpgradeApplyState::Upgrade { mut future, name } => {
                    match Future::poll(Pin::new(&mut future), cx) {
                        Poll::Pending => {
                            self.inner = InboundUpgradeApplyState::Upgrade { future, name };
                            return Poll::Pending;
                        }
                        Poll::Ready(Ok(x)) => {
                            tracing::trace!(upgrade=%name, "Upgraded inbound stream");
                            return Poll::Ready(Ok(x));
                        }
                        Poll::Ready(Err(e)) => {
                            tracing::debug!(upgrade=%name, "Failed to upgrade inbound stream");
                            return Poll::Ready(Err(UpgradeError::Apply(e)));
                        }
                    }
                }
                InboundUpgradeApplyState::Undefined => {
                    panic!("InboundUpgradeApplyState::poll called after completion")
                }
            }
        }
    }
}

/// Future returned by `apply_outbound`. Drives the upgrade process.
pub struct OutboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundConnectionUpgrade<Negotiated<C>>,
{
    inner: OutboundUpgradeApplyState<C, U>,
}

enum OutboundUpgradeApplyState<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundConnectionUpgrade<Negotiated<C>>,
{
    Init {
        future: DialerSelectFuture<C, <U::InfoIter as IntoIterator>::IntoIter>,
        upgrade: U,
    },
    Upgrade {
        future: Pin<Box<U::Future>>,
        name: String,
    },
    Undefined,
}

impl<C, U> Unpin for OutboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundConnectionUpgrade<Negotiated<C>>,
{
}

impl<C, U> Future for OutboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundConnectionUpgrade<Negotiated<C>>,
{
    type Output = Result<U::Output, UpgradeError<U::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match mem::replace(&mut self.inner, OutboundUpgradeApplyState::Undefined) {
                OutboundUpgradeApplyState::Init {
                    mut future,
                    upgrade,
                } => {
                    let (info, connection) = match Future::poll(Pin::new(&mut future), cx)? {
                        Poll::Ready(x) => x,
                        Poll::Pending => {
                            self.inner = OutboundUpgradeApplyState::Init { future, upgrade };
                            return Poll::Pending;
                        }
                    };
                    self.inner = OutboundUpgradeApplyState::Upgrade {
                        future: Box::pin(upgrade.upgrade_outbound(connection, info.clone())),
                        name: info.as_ref().to_owned(),
                    };
                }
                OutboundUpgradeApplyState::Upgrade { mut future, name } => {
                    match Future::poll(Pin::new(&mut future), cx) {
                        Poll::Pending => {
                            self.inner = OutboundUpgradeApplyState::Upgrade { future, name };
                            return Poll::Pending;
                        }
                        Poll::Ready(Ok(x)) => {
                            tracing::trace!(upgrade=%name, "Upgraded outbound stream");
                            return Poll::Ready(Ok(x));
                        }
                        Poll::Ready(Err(e)) => {
                            tracing::debug!(upgrade=%name, "Failed to upgrade outbound stream",);
                            return Poll::Ready(Err(UpgradeError::Apply(e)));
                        }
                    }
                }
                OutboundUpgradeApplyState::Undefined => {
                    panic!("OutboundUpgradeApplyState::poll called after completion")
                }
            }
        }
    }
}
