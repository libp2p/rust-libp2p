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
    Negotiated,
    transport::{
        Transport,
        TransportError,
        ListenerEvent,
        and_then::AndThen,
    },
    muxing::StreamMuxer,
    upgrade::{
        self,
        OutboundUpgrade,
        InboundUpgrade,
        apply_inbound,
        apply_outbound,
        UpgradeError,
        OutboundUpgradeApply,
        InboundUpgradeApply
    }
};
use futures::{prelude::*, ready};
use multiaddr::Multiaddr;
use std::{error::Error, fmt, pin::Pin, task::Context, task::Poll};

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
        AndThen<T, impl FnOnce(C, ConnectedPoint) -> Authenticate<C, U> + Clone>
    > where
        T: Transport<Output = C>,
        I: ConnectionInfo,
        C: AsyncRead + AsyncWrite + Unpin,
        D: AsyncRead + AsyncWrite + Unpin,
        U: InboundUpgrade<Negotiated<C>, Output = (I, D), Error = E>,
        U: OutboundUpgrade<Negotiated<C>, Output = (I, D), Error = E> + Clone,
        E: Error + 'static,
    {
        let version = self.version;
        Builder::new(self.inner.and_then(move |conn, endpoint| {
            Authenticate {
                inner: upgrade::apply(conn, upgrade, endpoint, version)
            }
        }), version)
    }

    /// Applies an arbitrary upgrade on an authenticated, non-multiplexed
    /// transport.
    ///
    /// The upgrade receives the I/O resource (i.e. connection) `C` and
    /// must produce a new I/O resource `D`. Any number of such upgrades
    /// can be performed.
    ///
    /// ## Transitions
    ///
    ///   * I/O upgrade: `C -> D`.
    ///   * Transport output: `(I, C) -> (I, D)`.
    pub fn apply<C, D, U, I, E>(self, upgrade: U) -> Builder<Upgrade<T, U>>
    where
        T: Transport<Output = (I, C)>,
        C: AsyncRead + AsyncWrite + Unpin,
        D: AsyncRead + AsyncWrite + Unpin,
        I: ConnectionInfo,
        U: InboundUpgrade<Negotiated<C>, Output = D, Error = E>,
        U: OutboundUpgrade<Negotiated<C>, Output = D, Error = E> + Clone,
        E: Error + 'static,
    {
        Builder::new(Upgrade::new(self.inner, upgrade), self.version)
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
        -> AndThen<T, impl FnOnce((I, C), ConnectedPoint) -> Multiplex<C, U, I> + Clone>
    where
        T: Transport<Output = (I, C)>,
        C: AsyncRead + AsyncWrite + Unpin,
        M: StreamMuxer,
        I: ConnectionInfo,
        U: InboundUpgrade<Negotiated<C>, Output = M, Error = E>,
        U: OutboundUpgrade<Negotiated<C>, Output = M, Error = E> + Clone,
        E: Error + 'static,
    {
        let version = self.version;
        self.inner.and_then(move |(i, c), endpoint| {
            let upgrade = upgrade::apply(c, upgrade, endpoint, version);
            Multiplex { info: Some(i), upgrade }
        })
    }
}

/// An upgrade that authenticates the remote peer, typically
/// in the context of negotiating a secure channel.
///
/// Configured through [`Builder::authenticate`].
#[pin_project::pin_project]
pub struct Authenticate<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>> + OutboundUpgrade<Negotiated<C>>
{
    #[pin]
    inner: EitherUpgrade<C, U>
}

impl<C, U> Future for Authenticate<C, U>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>> + OutboundUpgrade<Negotiated<C>,
        Output = <U as InboundUpgrade<Negotiated<C>>>::Output,
        Error = <U as InboundUpgrade<Negotiated<C>>>::Error
    >
{
    type Output = <EitherUpgrade<C, U> as Future>::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();
        Future::poll(this.inner, cx)
    }
}

/// An upgrade that negotiates a (sub)stream multiplexer on
/// top of an authenticated transport.
///
/// Configured through [`Builder::multiplex`].
#[pin_project::pin_project]
pub struct Multiplex<C, U, I>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>> + OutboundUpgrade<Negotiated<C>>,
{
    info: Option<I>,
    #[pin]
    upgrade: EitherUpgrade<C, U>,
}

impl<C, U, I, M, E> Future for Multiplex<C, U, I>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>, Output = M, Error = E>,
    U: OutboundUpgrade<Negotiated<C>, Output = M, Error = E>
{
    type Output = Result<(I, M), UpgradeError<E>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();
        let m = match ready!(Future::poll(this.upgrade, cx)) {
            Ok(m) => m,
            Err(err) => return Poll::Ready(Err(err)),
        };
        let i = this.info.take().expect("Multiplex future polled after completion.");
        Poll::Ready(Ok((i, m)))
    }
}

/// An inbound or outbound upgrade.
type EitherUpgrade<C, U> = future::Either<InboundUpgradeApply<C, U>, OutboundUpgradeApply<C, U>>;

/// An upgrade on an authenticated, non-multiplexed [`Transport`].
///
/// See [`Transport::upgrade`]
#[derive(Debug, Copy, Clone)]
pub struct Upgrade<T, U> { inner: T, upgrade: U }

impl<T, U> Upgrade<T, U> {
    pub fn new(inner: T, upgrade: U) -> Self {
        Upgrade { inner, upgrade }
    }
}

impl<T, C, D, U, I, E> Transport for Upgrade<T, U>
where
    T: Transport<Output = (I, C)>,
    T::Error: 'static,
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>, Output = D, Error = E>,
    U: OutboundUpgrade<Negotiated<C>, Output = D, Error = E> + Clone,
    E: Error + 'static
{
    type Output = (I, D);
    type Error = TransportUpgradeError<T::Error, E>;
    type Listener = ListenerStream<T::Listener, U>;
    type ListenerUpgrade = ListenerUpgradeFuture<T::ListenerUpgrade, U, I, C>;
    type Dial = DialUpgradeFuture<T::Dial, U, I, C>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let future = self.inner.dial(addr.clone())
            .map_err(|err| err.map(TransportUpgradeError::Transport))?;
        Ok(DialUpgradeFuture {
            future: Box::pin(future),
            upgrade: future::Either::Left(Some(self.upgrade))
        })
    }

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let stream = self.inner.listen_on(addr)
            .map_err(|err| err.map(TransportUpgradeError::Transport))?;
        Ok(ListenerStream {
            stream: Box::pin(stream),
            upgrade: self.upgrade
        })
    }
}

/// Errors produced by a transport upgrade.
#[derive(Debug)]
pub enum TransportUpgradeError<T, U> {
    /// Error in the transport.
    Transport(T),
    /// Error while upgrading to a protocol.
    Upgrade(UpgradeError<U>),
}

impl<T, U> fmt::Display for TransportUpgradeError<T, U>
where
    T: fmt::Display,
    U: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportUpgradeError::Transport(e) => write!(f, "Transport error: {}", e),
            TransportUpgradeError::Upgrade(e) => write!(f, "Upgrade error: {}", e),
        }
    }
}

impl<T, U> Error for TransportUpgradeError<T, U>
where
    T: Error + 'static,
    U: Error + 'static,
{
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            TransportUpgradeError::Transport(e) => Some(e),
            TransportUpgradeError::Upgrade(e) => Some(e),
        }
    }
}

/// The [`Transport::Dial`] future of an [`Upgrade`]d transport.
pub struct DialUpgradeFuture<F, U, I, C>
where
    U: OutboundUpgrade<Negotiated<C>>,
    C: AsyncRead + AsyncWrite + Unpin,
{
    future: Pin<Box<F>>,
    upgrade: future::Either<Option<U>, (Option<I>, OutboundUpgradeApply<C, U>)>
}

impl<F, U, I, C, D> Future for DialUpgradeFuture<F, U, I, C>
where
    F: TryFuture<Ok = (I, C)>,
    C: AsyncRead + AsyncWrite + Unpin,
    U: OutboundUpgrade<Negotiated<C>, Output = D>,
    U::Error: Error
{
    type Output = Result<(I, D), TransportUpgradeError<F::Error, U::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        // We use a `this` variable because the compiler can't mutably borrow multiple times
        // accross a `Deref`.
        let this = &mut *self;

        loop {
            this.upgrade = match this.upgrade {
                future::Either::Left(ref mut up) => {
                    let (i, c) = match ready!(TryFuture::try_poll(this.future.as_mut(), cx).map_err(TransportUpgradeError::Transport)) {
                        Ok(v) => v,
                        Err(err) => return Poll::Ready(Err(err)),
                    };
                    let u = up.take().expect("DialUpgradeFuture is constructed with Either::Left(Some).");
                    future::Either::Right((Some(i), apply_outbound(c, u, upgrade::Version::V1)))
                }
                future::Either::Right((ref mut i, ref mut up)) => {
                    let d = match ready!(Future::poll(Pin::new(up), cx).map_err(TransportUpgradeError::Upgrade)) {
                        Ok(d) => d,
                        Err(err) => return Poll::Ready(Err(err)),
                    };
                    let i = i.take().expect("DialUpgradeFuture polled after completion.");
                    return Poll::Ready(Ok((i, d)))
                }
            }
        }
    }
}

impl<F, U, I, C> Unpin for DialUpgradeFuture<F, U, I, C>
where
    U: OutboundUpgrade<Negotiated<C>>,
    C: AsyncRead + AsyncWrite + Unpin,
{
}

/// The [`Transport::Listener`] stream of an [`Upgrade`]d transport.
pub struct ListenerStream<S, U> {
    stream: Pin<Box<S>>,
    upgrade: U
}

impl<S, U, F, I, C, D, E> Stream for ListenerStream<S, U>
where
    S: TryStream<Ok = ListenerEvent<F, E>, Error = E>,
    F: TryFuture<Ok = (I, C)>,
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>, Output = D> + Clone
{
    type Item = Result<ListenerEvent<ListenerUpgradeFuture<F, U, I, C>, TransportUpgradeError<E, U::Error>>, TransportUpgradeError<E, U::Error>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match ready!(TryStream::try_poll_next(self.stream.as_mut(), cx)) {
            Some(Ok(event)) => {
                let event = event
                    .map(move |future| {
                        ListenerUpgradeFuture {
                            future: Box::pin(future),
                            upgrade: future::Either::Left(Some(self.upgrade.clone()))
                        }
                    })
                    .map_err(TransportUpgradeError::Transport);
                Poll::Ready(Some(Ok(event)))
            }
            Some(Err(err)) => {
                Poll::Ready(Some(Err(TransportUpgradeError::Transport(err))))
            }
            None => Poll::Ready(None)
        }
    }
}

impl<S, U> Unpin for ListenerStream<S, U> {
}

/// The [`Transport::ListenerUpgrade`] future of an [`Upgrade`]d transport.
pub struct ListenerUpgradeFuture<F, U, I, C>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>>
{
    future: Pin<Box<F>>,
    upgrade: future::Either<Option<U>, (Option<I>, InboundUpgradeApply<C, U>)>
}

impl<F, U, I, C, D> Future for ListenerUpgradeFuture<F, U, I, C>
where
    F: TryFuture<Ok = (I, C)>,
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>, Output = D>,
    U::Error: Error
{
    type Output = Result<(I, D), TransportUpgradeError<F::Error, U::Error>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        // We use a `this` variable because the compiler can't mutably borrow multiple times
        // accross a `Deref`.
        let this = &mut *self;

        loop {
            this.upgrade = match this.upgrade {
                future::Either::Left(ref mut up) => {
                    let (i, c) = match ready!(TryFuture::try_poll(this.future.as_mut(), cx).map_err(TransportUpgradeError::Transport)) {
                        Ok(v) => v,
                        Err(err) => return Poll::Ready(Err(err))
                    };
                    let u = up.take().expect("ListenerUpgradeFuture is constructed with Either::Left(Some).");
                    future::Either::Right((Some(i), apply_inbound(c, u)))
                }
                future::Either::Right((ref mut i, ref mut up)) => {
                    let d = match ready!(TryFuture::try_poll(Pin::new(up), cx).map_err(TransportUpgradeError::Upgrade)) {
                        Ok(v) => v,
                        Err(err) => return Poll::Ready(Err(err))
                    };
                    let i = i.take().expect("ListenerUpgradeFuture polled after completion.");
                    return Poll::Ready(Ok((i, d)))
                }
            }
        }
    }
}

impl<F, U, I, C> Unpin for ListenerUpgradeFuture<F, U, I, C>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: InboundUpgrade<Negotiated<C>>
{
}
