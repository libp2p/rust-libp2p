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

use crate::Negotiated;
use crate::upgrade::{Role, Upgrade, UpgradeError, ProtocolName};
use futures::prelude::*;
use log::debug;
use multistream_select::{self, DialerSelectFuture, ListenerSelectFuture};
use std::{iter, mem, pin::Pin, task::Context, task::Poll};

pub use multistream_select::Version;

/// Applies an upgrade to the inbound and outbound direction of a connection or substream.
pub fn apply<C, U>(conn: C, upgrade: U, role: Role, v: Version) -> UpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>
{
    let iter = upgrade.protocol_info().into_iter().map(NameWrap as fn(_) -> NameWrap<_>);
    let inner = if role == Role::Responder {
        UpgradeApplyState::ListenerSelect {
            future: multistream_select::listener_select_proto(conn, iter),
            upgrade,
        }
    } else {
        UpgradeApplyState::DialerSelect {
            future: multistream_select::dialer_select_proto(conn, iter, v),
            upgrade,
        }
    };
    UpgradeApply { inner }
}

/// Future returned by `apply`. Drives the upgrade process.
pub struct UpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>
{
    inner: UpgradeApplyState<C, U>
}

enum UpgradeApplyState<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
    ListenerSelect {
        future: ListenerSelectFuture<C, NameWrap<U::Info>>,
        upgrade: U,
    },
    DialerSelect {
        future: DialerSelectFuture<C, NameWrapIter<<U::InfoIter as IntoIterator>::IntoIter>>,
        upgrade: U,
    },
    Upgrade {
        future: Pin<Box<U::Future>>
    },
    Undefined
}

impl<C, U> Unpin for UpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
}

impl<C, U> Future for UpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: Upgrade<Negotiated<C>>,
{
    type Output = Result<U::Output, UpgradeError<U::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match mem::replace(&mut self.inner, UpgradeApplyState::Undefined) {
                UpgradeApplyState::ListenerSelect { mut future, upgrade } => {
                    let (info, io) = match Future::poll(Pin::new(&mut future), cx)? {
                        Poll::Ready(x) => x,
                        Poll::Pending => {
                            self.inner = UpgradeApplyState::ListenerSelect { future, upgrade };
                            return Poll::Pending
                        }
                    };
                    self.inner = UpgradeApplyState::Upgrade {
                        future: Box::pin(upgrade.upgrade(io, info.0, Role::Responder))
                    };
                }
                UpgradeApplyState::DialerSelect { mut future, upgrade } => {
                    let (info, connection) = match Future::poll(Pin::new(&mut future), cx)? {
                        Poll::Ready(x) => x,
                        Poll::Pending => {
                            self.inner = UpgradeApplyState::DialerSelect { future, upgrade };
                            return Poll::Pending
                        }
                    };
                    self.inner = UpgradeApplyState::Upgrade {
                        future: Box::pin(upgrade.upgrade(connection, info.0, Role::Initiator))
                    };
                }
                UpgradeApplyState::Upgrade { mut future } => {
                    match Future::poll(Pin::new(&mut future), cx) {
                        Poll::Pending => {
                            self.inner = UpgradeApplyState::Upgrade { future };
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
                UpgradeApplyState::Undefined =>
                    panic!("InboundUpgradeApplyState::poll called after completion")
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
