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

use crate::upgrade::{InboundUpgrade, OutboundUpgrade, ProtocolName, UpgradeError};
use crate::{connection::ConnectedPoint, Negotiated};
use futures::{future::Either, prelude::*};
use log::debug;
use multistream_select::{self, DialerSelectFuture, ListenerSelectFuture};
use std::{iter, mem, pin::Pin, task::Context, task::Poll};

pub use multistream_select::Version;
use smallvec::SmallVec;
use std::fmt;

// TODO: Still needed?
/// Applies an upgrade to the inbound and outbound direction of a connection or substream.
pub fn apply<C, U>(
    conn: C,
    up: U,
    cp: ConnectedPoint,
    v: Version,
) -> Either<InboundUpgradeApply<C, U>, OutboundUpgradeApply<C, U>>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>> + OutboundUpgrade<Negotiated<C>>,
{
    match cp {
        ConnectedPoint::Dialer { role_override, .. } if role_override.is_dialer() => {
            Either::Right(apply_outbound(conn, up, v))
        }
        _ => Either::Left(apply_inbound(conn, up)),
    }
}

/// Tries to perform an upgrade on an inbound connection or substream.
pub fn apply_inbound<C, U>(conn: C, up: U) -> InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>>,
{
    let iter = up
        .protocol_info()
        .into_iter()
        .map(NameWrap as fn(_) -> NameWrap<_>);
    let future = multistream_select::listener_select_proto(conn, iter);
    InboundUpgradeApply {
        inner: InboundUpgradeApplyState::Init {
            future,
            upgrade: up,
        },
    }
}

/// Tries to perform an upgrade on an outbound connection or substream.
pub fn apply_outbound<C, U>(conn: C, up: U, v: Version) -> OutboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundUpgrade<Negotiated<C>>,
{
    let iter = up
        .protocol_info()
        .into_iter()
        .map(NameWrap as fn(_) -> NameWrap<_>);
    let future = multistream_select::dialer_select_proto(conn, iter, v);
    OutboundUpgradeApply {
        inner: OutboundUpgradeApplyState::Init {
            future,
            upgrade: up,
        },
    }
}

/// Future returned by `apply_inbound`. Drives the upgrade process.
pub struct InboundUpgradeApply<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>>,
{
    inner: InboundUpgradeApplyState<C, U>,
}

enum InboundUpgradeApplyState<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>>,
{
    Init {
        future: ListenerSelectFuture<C, NameWrap<U::Info>>,
        upgrade: U,
    },
    Upgrade {
        future: Pin<Box<U::Future>>,
        name: SmallVec<[u8; 32]>,
    },
    Undefined,
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
                    let name = SmallVec::from_slice(info.protocol_name());
                    self.inner = InboundUpgradeApplyState::Upgrade {
                        future: Box::pin(upgrade.upgrade_inbound(io, info.0)),
                        name,
                    };
                }
                InboundUpgradeApplyState::Upgrade { mut future, name } => {
                    match Future::poll(Pin::new(&mut future), cx) {
                        Poll::Pending => {
                            self.inner = InboundUpgradeApplyState::Upgrade { future, name };
                            return Poll::Pending;
                        }
                        Poll::Ready(Ok(x)) => {
                            log::trace!("Upgraded inbound stream to {}", DisplayProtocolName(name));
                            return Poll::Ready(Ok(x));
                        }
                        Poll::Ready(Err(e)) => {
                            debug!(
                                "Failed to upgrade inbound stream to {}",
                                DisplayProtocolName(name)
                            );
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
    U: OutboundUpgrade<Negotiated<C>>,
{
    inner: OutboundUpgradeApplyState<C, U>,
}

enum OutboundUpgradeApplyState<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundUpgrade<Negotiated<C>>,
{
    Init {
        future: DialerSelectFuture<C, NameWrapIter<<U::InfoIter as IntoIterator>::IntoIter>>,
        upgrade: U,
    },
    Upgrade {
        future: Pin<Box<U::Future>>,
        name: SmallVec<[u8; 32]>,
    },
    Undefined,
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
                    let name = SmallVec::from_slice(info.protocol_name());
                    self.inner = OutboundUpgradeApplyState::Upgrade {
                        future: Box::pin(upgrade.upgrade_outbound(connection, info.0)),
                        name,
                    };
                }
                OutboundUpgradeApplyState::Upgrade { mut future, name } => {
                    match Future::poll(Pin::new(&mut future), cx) {
                        Poll::Pending => {
                            self.inner = OutboundUpgradeApplyState::Upgrade { future, name };
                            return Poll::Pending;
                        }
                        Poll::Ready(Ok(x)) => {
                            log::trace!(
                                "Upgraded outbound stream to {}",
                                DisplayProtocolName(name)
                            );
                            return Poll::Ready(Ok(x));
                        }
                        Poll::Ready(Err(e)) => {
                            debug!(
                                "Failed to upgrade outbound stream to {}",
                                DisplayProtocolName(name)
                            );
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

type NameWrapIter<I> = iter::Map<I, fn(<I as Iterator>::Item) -> NameWrap<<I as Iterator>::Item>>;

/// Wrapper type to expose an `AsRef<[u8]>` impl for all types implementing `ProtocolName`.
#[derive(Clone)]
struct NameWrap<N>(N);

impl<N: ProtocolName> AsRef<[u8]> for NameWrap<N> {
    fn as_ref(&self) -> &[u8] {
        self.0.protocol_name()
    }
}

/// Wrapper for printing a [`ProtocolName`] that is expected to be mostly ASCII
pub(crate) struct DisplayProtocolName<N>(pub N);

impl<N: ProtocolName> fmt::Display for DisplayProtocolName<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use fmt::Write;
        for byte in self.0.protocol_name() {
            if (b' '..=b'~').contains(byte) {
                f.write_char(char::from(*byte))?;
            } else {
                write!(f, "<{:02X}>", byte)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_protocol_name() {
        assert_eq!(DisplayProtocolName(b"/hello/1.0").to_string(), "/hello/1.0");
        assert_eq!(DisplayProtocolName("/hellö/").to_string(), "/hell<C3><B6>/");
        assert_eq!(
            DisplayProtocolName((0u8..=255).collect::<Vec<_>>()).to_string(),
            (0..32)
                .map(|c| format!("<{:02X}>", c))
                .chain((32..127).map(|c| format!("{}", char::from_u32(c).unwrap())))
                .chain((127..256).map(|c| format!("<{:02X}>", c)))
                .collect::<String>()
        );
    }
}
