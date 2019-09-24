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

use crate::ConnectedPoint;
use crate::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeError};
use crate::upgrade::ProtocolName;
use futures::{future::Either, prelude::*};
use log::debug;
use multistream_select::{self, DialerSelectFuture, ListenerSelectFuture};
use std::{iter, mem};
use tokio_io::{AsyncRead, AsyncWrite};

pub use multistream_select::Version;

/// Applies an upgrade to the inbound and outbound direction of a connection or substream.
pub fn apply<C, U>(conn: C, up: U, cp: ConnectedPoint, v: Version)
    -> Either<InboundUpgradeApply<C, U>, OutboundUpgradeApply<C, U>>
where
    C: AsyncRead + AsyncWrite,
    U: InboundUpgrade<C> + OutboundUpgrade<C>,
{
    if cp.is_listener() {
        Either::A(apply_inbound(conn, up))
    } else {
        Either::B(apply_outbound(conn, up, v))
    }
}

/// Tries to perform an upgrade on an inbound connection or substream.
pub fn apply_inbound<C, U>(conn: C, up: U) -> InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite,
    U: InboundUpgrade<C>,
{
    let iter = up.protocol_info().into_iter().map(NameWrap as fn(_) -> NameWrap<_>);
    let future = multistream_select::listener_select_proto(conn, iter);
    InboundUpgradeApply {
        inner: InboundUpgradeApplyState::Init { future, upgrade: up }
    }
}

/// Tries to perform an upgrade on an outbound connection or substream.
pub fn apply_outbound<C, U>(conn: C, up: U, v: Version) -> OutboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite,
    U: OutboundUpgrade<C>
{
    let iter = up.protocol_info().into_iter().map(NameWrap as fn(_) -> NameWrap<_>);
    let future = multistream_select::dialer_select_proto(conn, iter, v);
    OutboundUpgradeApply {
        inner: OutboundUpgradeApplyState::Init { future, upgrade: up }
    }
}

/// Future returned by `apply_inbound`. Drives the upgrade process.
pub struct InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite,
    U: InboundUpgrade<C>
{
    inner: InboundUpgradeApplyState<C, U>
}

enum InboundUpgradeApplyState<C, U>
where
    C: AsyncRead + AsyncWrite,
    U: InboundUpgrade<C>,
{
    Init {
        future: ListenerSelectFuture<C, NameWrap<U::Info>>,
        upgrade: U,
    },
    Upgrade {
        future: U::Future
    },
    Undefined
}

impl<C, U> Future for InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite,
    U: InboundUpgrade<C>,
{
    type Item = U::Output;
    type Error = UpgradeError<U::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.inner, InboundUpgradeApplyState::Undefined) {
                InboundUpgradeApplyState::Init { mut future, upgrade } => {
                    let (info, io) = match future.poll()? {
                        Async::Ready(x) => x,
                        Async::NotReady => {
                            self.inner = InboundUpgradeApplyState::Init { future, upgrade };
                            return Ok(Async::NotReady)
                        }
                    };
                    self.inner = InboundUpgradeApplyState::Upgrade {
                        future: upgrade.upgrade_inbound(io, info.0)
                    };
                }
                InboundUpgradeApplyState::Upgrade { mut future } => {
                    match future.poll() {
                        Ok(Async::NotReady) => {
                            self.inner = InboundUpgradeApplyState::Upgrade { future };
                            return Ok(Async::NotReady)
                        }
                        Ok(Async::Ready(x)) => {
                            debug!("Successfully applied negotiated protocol");
                            return Ok(Async::Ready(x))
                        }
                        Err(e) => {
                            debug!("Failed to apply negotiated protocol");
                            return Err(UpgradeError::Apply(e))
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
    C: AsyncRead + AsyncWrite,
    U: OutboundUpgrade<C>
{
    inner: OutboundUpgradeApplyState<C, U>
}

enum OutboundUpgradeApplyState<C, U>
where
    C: AsyncRead + AsyncWrite,
    U: OutboundUpgrade<C>
{
    Init {
        future: DialerSelectFuture<C, NameWrapIter<<U::InfoIter as IntoIterator>::IntoIter>>,
        upgrade: U
    },
    Upgrade {
        future: U::Future
    },
    Undefined
}

impl<C, U> Future for OutboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite,
    U: OutboundUpgrade<C>
{
    type Item = U::Output;
    type Error = UpgradeError<U::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.inner, OutboundUpgradeApplyState::Undefined) {
                OutboundUpgradeApplyState::Init { mut future, upgrade } => {
                    let (info, connection) = match future.poll()? {
                        Async::Ready(x) => x,
                        Async::NotReady => {
                            self.inner = OutboundUpgradeApplyState::Init { future, upgrade };
                            return Ok(Async::NotReady)
                        }
                    };
                    self.inner = OutboundUpgradeApplyState::Upgrade {
                        future: upgrade.upgrade_outbound(connection, info.0)
                    };
                }
                OutboundUpgradeApplyState::Upgrade { mut future } => {
                    match future.poll() {
                        Ok(Async::NotReady) => {
                            self.inner = OutboundUpgradeApplyState::Upgrade { future };
                            return Ok(Async::NotReady)
                        }
                        Ok(Async::Ready(x)) => {
                            debug!("Successfully applied negotiated protocol");
                            return Ok(Async::Ready(x))
                        }
                        Err(e) => {
                            debug!("Failed to apply negotiated protocol");
                            return Err(UpgradeError::Apply(e))
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

