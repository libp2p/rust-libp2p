// Copyright 2017-2019 Parity Technologies (UK) Ltd.
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

//! Configuration of transport protocol upgrades.

pub use crate::upgrade::Version;

use crate::{
    ConnectedPoint,
    ConnectionInfo,
    transport::{
        Transport,
        and_then::AndThen,
    },
    muxing::StreamMuxer,
    upgrade::{
        self,
        OutboundUpgrade,
        InboundUpgrade,
        UpgradeError,
    }
};
use futures::prelude::*;
use std::{error::Error, pin::Pin};

/// A `Builder` facilitates upgrading of a [`Transport`] for use with
/// a [`Network`].
///
/// The upgrade process is defined by the following stages:
///
///    [`authenticate`](Builder::authenticate)`{1}`
/// -> [`apply`](Builder::apply)`{*}`
/// -> [`multiplex`](Builder::multiplex)`{1}`
///
/// It thus enforces the following invariants on every transport
/// obtained from [`multiplex`](Builder::multiplex):
///
///   1. The transport must be [authenticated](Builder::authenticate)
///      and [multiplexed](Builder::multiplex).
///   2. Authentication must precede the negotiation of a multiplexer.
///   3. Applying a multiplexer is the last step in the upgrade process.
///   4. The [`Transport::Output`] conforms to the requirements of a [`Network`],
///      namely a tuple of a [`ConnectionInfo`] (from the authentication upgrade) and a
///      [`StreamMuxer`] (from the multiplexing upgrade).
///
/// [`Network`]: crate::nodes::Network
pub struct Builder<T> {
    inner: T,
    version: upgrade::Version,
}

impl<T> Builder<T>
where
    T: Transport,
    T::Error: 'static,
{
    /// Creates a `Builder` over the given (base) `Transport`.
    pub fn new(inner: T, version: upgrade::Version) -> Builder<T> {
        Builder { inner, version }
    }

    /// Upgrades the transport to perform authentication of the remote.
    ///
    /// The supplied upgrade receives the I/O resource `C` and must
    /// produce a pair `(I, D)`, where `I` is a [`ConnectionInfo`] and
    /// `D` is a new I/O resource. The upgrade must thus at a minimum
    /// identify the remote, which typically involves the use of a
    /// cryptographic authentication protocol in the context of establishing
    /// a secure channel.
    ///
    /// ## Transitions
    ///
    ///   * I/O upgrade: `C -> (I, D)`.
    ///   * Transport output: `C -> (I, D)`
    pub fn authenticate<C, D, U, I, E>(self, upgrade: U) -> Builder<
        AndThen<T, impl FnOnce(C, ConnectedPoint) -> Pin<Box<dyn Future<Output = Result<(I, D), UpgradeError<E>>> + Send>> + Clone>
    > where
        T: Transport<Output = C>,
        T::Dial: Unpin,
        T::Listener: Unpin,
        T::ListenerUpgrade: Unpin,
        I: ConnectionInfo + 'static,
        C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
        D: AsyncRead + AsyncWrite + Send + Unpin + 'static,
        U: InboundUpgrade<C, Output = (I, D), Error = E> + Send + 'static,
        <U as InboundUpgrade<C>>::Future: Send + 'static,
        U: OutboundUpgrade<C, Output = (I, D), Error = E> + Clone,
        <U as OutboundUpgrade<C>>::Future: Send + 'static,
        U::Info: Send + 'static,
        U::InfoIter: Send + 'static,
        <U::InfoIter as IntoIterator>::IntoIter: Send + 'static,
        E: Error + 'static,
    {
        let version = self.version;
        Builder::new(self.inner.and_then(move |conn, endpoint| {
            Box::pin(upgrade::apply(conn, upgrade, endpoint, version)) as Pin<Box<_>>
        }), version)
    }

    /// Upgrades the transport with a (sub)stream multiplexer.
    ///
    /// The supplied upgrade receives the I/O resource `C` and must
    /// produce a [`StreamMuxer`] `M`. The transport must already be authenticated.
    /// This ends the (regular) transport upgrade process, yielding the underlying,
    /// configured transport.
    ///
    /// ## Transitions
    ///
    ///   * I/O upgrade: `C -> M`.
    ///   * Transport output: `(I, C) -> (I, M)`.
    pub fn multiplex<C, M, U, I, E>(self, upgrade: U)
    -> AndThen<T, impl FnOnce((I, C), ConnectedPoint) -> Pin<Box<dyn Future<Output = Result<(I, M), UpgradeError<E>>> + Send>> + Clone>
    where
        T: Transport<Output = (I, C)>,
        T::Dial: Unpin,
        T::Listener: Unpin,
        T::ListenerUpgrade: Unpin,
        C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
        M: StreamMuxer,
        I: ConnectionInfo + Send + 'static,
        U: InboundUpgrade<C, Output = M, Error = E> + Send + 'static,
        <U as InboundUpgrade<C>>::Future: Send + 'static,
        U: OutboundUpgrade<C, Output = M, Error = E> + Clone,
        <U as OutboundUpgrade<C>>::Future: Send + 'static,
        U::Info: Send + 'static,
        U::InfoIter: Send + 'static,
        <U::InfoIter as IntoIterator>::IntoIter: Send + 'static,
        E: Error + 'static,
    {
        let version = self.version;
        self.inner.and_then(move |(i, c), endpoint| {
            Box::pin(async move {
                let upgrade = upgrade::apply(c, upgrade, endpoint, version).await?;
                Ok((i, upgrade))
            }) as Pin<Box<_>>
        })
    }
}
