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

use bytes::Bytes;
use futures::{prelude::*, future::Either};
use multistream_select::{self, DialerSelectFuture, ListenerSelectFuture};
use std::{io::{Error as IoError, ErrorKind as IoErrorKind}, mem};
use tokio_io::{AsyncRead, AsyncWrite};
use upgrade::{ConnectionUpgrade, Endpoint};

/// Applies a connection upgrade on a socket.
///
/// Returns a `Future` that returns the outcome of the connection upgrade.
#[inline]
pub fn apply<C, U, Maf>(conn: C, upgrade: U, e: Endpoint, remote: Maf) -> UpgradeApplyFuture<C, U, Maf>
where
    U: ConnectionUpgrade<C, Maf>,
    U::NamesIter: Clone, // TODO: not elegant
    C: AsyncRead + AsyncWrite,
{
    UpgradeApplyFuture::Init {
        future: negotiate(conn, &upgrade, e),
        upgrade,
        endpoint: e,
        remote
    }
}


pub enum UpgradeApplyFuture<C, U, Maf>
where
    U: ConnectionUpgrade<C, Maf>,
    C: AsyncRead + AsyncWrite
{
    Init {
        future: NegotiationFuture<C, ProtocolNames<U::NamesIter>, U::UpgradeIdentifier>,
        upgrade: U,
        endpoint: Endpoint,
        remote: Maf
    },
    Upgrade {
        future: U::Future
    },
    Undefined
}

impl<C, U, Maf> Future for UpgradeApplyFuture<C, U, Maf>
where
    U: ConnectionUpgrade<C, Maf>,
    U::NamesIter: Clone,
    C: AsyncRead + AsyncWrite
{
    type Item = (U::Output, U::MultiaddrFuture);
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(self, UpgradeApplyFuture::Undefined) {
                UpgradeApplyFuture::Init { mut future, upgrade, endpoint, remote } => {
                    let (upgrade_id, connection) = match future.poll()? {
                        Async::Ready(x) => x,
                        Async::NotReady => {
                            *self = UpgradeApplyFuture::Init { future, upgrade, endpoint, remote };
                            return Ok(Async::NotReady)
                        }
                    };
                    *self = UpgradeApplyFuture::Upgrade {
                        future: upgrade.upgrade(connection, upgrade_id, endpoint, remote)
                    };
                }
                UpgradeApplyFuture::Upgrade { mut future } => {
                    match future.poll() {
                        Ok(Async::NotReady) => {
                            *self = UpgradeApplyFuture::Upgrade { future };
                            return Ok(Async::NotReady)
                        }
                        Ok(Async::Ready(x)) => {
                            debug!("Successfully applied negotiated protocol");
                            return Ok(Async::Ready(x))
                        }
                        Err(e) => {
                            debug!("Failed to apply negotiated protocol: {:?}", e);
                            return Err(e)
                        }
                    }
                }
                UpgradeApplyFuture::Undefined =>
                    panic!("UpgradeApplyFuture::poll called after completion")
            }
        }
    }
}


/// Negotiates a protocol on a stream.
///
/// Returns a `Future` that returns the negotiated protocol and the stream.
#[inline]
pub fn negotiate<C, I, U, Maf>(
    connection: C,
    upgrade: &U,
    endpoint: Endpoint,
) -> NegotiationFuture<C, ProtocolNames<U::NamesIter>, U::UpgradeIdentifier>
where
    U: ConnectionUpgrade<I, Maf>,
    U::NamesIter: Clone, // TODO: not elegant
    C: AsyncRead + AsyncWrite,
{
    debug!("Starting protocol negotiation");
    let iter = ProtocolNames(upgrade.protocol_names());
    NegotiationFuture {
        inner: match endpoint {
            Endpoint::Listener => Either::A(multistream_select::listener_select_proto(connection, iter)),
            Endpoint::Dialer => Either::B(multistream_select::dialer_select_proto(connection, iter)),
        }
    }
}


pub struct NegotiationFuture<R: AsyncRead + AsyncWrite, I, P> {
    inner: Either<ListenerSelectFuture<R, I, P>, DialerSelectFuture<R, I, P>>
}

impl<R, I, M, P> Future for NegotiationFuture<R, I, P>
where
    R: AsyncRead + AsyncWrite,
    I: Iterator<Item=(Bytes, M, P)> + Clone,
    M: FnMut(&Bytes, &Bytes) -> bool,
{
    type Item = (P, R);
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(x)) => {
                debug!("Successfully negotiated protocol upgrade");
                Ok(Async::Ready(x))
            }
            Err(e) => {
                let err = IoError::new(IoErrorKind::Other, e);
                debug!("Error while negotiated protocol upgrade: {:?}", err);
                Err(err)
            }
        }
    }
}


#[derive(Clone)]
pub struct ProtocolNames<I>(I);

impl<I, Id> Iterator for ProtocolNames<I>
where
    I: Iterator<Item=(Bytes, Id)>
{
    type Item = (Bytes, fn(&Bytes, &Bytes) -> bool, Id);

    fn next(&mut self) -> Option<Self::Item> {
        let f = <Bytes as PartialEq>::eq as fn(&Bytes, &Bytes) -> bool;
        self.0.next().map(|(b, id)| (b, f, id))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}


