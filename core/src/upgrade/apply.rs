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
use futures::{prelude::*, future};
use multistream_select;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use tokio_io::{AsyncRead, AsyncWrite};
use upgrade::{ConnectionUpgrade, Endpoint};

/// Applies a connection upgrade on a socket.
///
/// Returns a `Future` that returns the outcome of the connection upgrade.
#[inline]
pub fn apply<C, U, Maf>(
    connection: C,
    upgrade: U,
    endpoint: Endpoint,
    remote_addr: Maf,
) -> impl Future<Item = (U::Output, U::MultiaddrFuture), Error = IoError>
where
    U: ConnectionUpgrade<C, Maf>,
    U::NamesIter: Clone, // TODO: not elegant
    C: AsyncRead + AsyncWrite,
{
    negotiate(connection, &upgrade, endpoint)
        .and_then(move |(upgrade_id, connection)| {
            upgrade.upgrade(connection, upgrade_id, endpoint, remote_addr)
        })
        .into_future()
        .then(|val| {
            match val {
                Ok(_) => debug!("Successfully applied negotiated protocol"),
                Err(ref err) => debug!("Failed to apply negotiated protocol: {:?}", err),
            }
            val
        })
}

/// Negotiates a protocol on a stream.
///
/// Returns a `Future` that returns the negotiated protocol and the stream.
#[inline]
pub fn negotiate<C, I, U, Maf>(
    connection: C,
    upgrade: &U,
    endpoint: Endpoint,
) -> impl Future<Item = (U::UpgradeIdentifier, C), Error = IoError>
where
    U: ConnectionUpgrade<I, Maf>,
    U::NamesIter: Clone, // TODO: not elegant
    C: AsyncRead + AsyncWrite,
{
    let iter = upgrade
        .protocol_names()
        .map::<_, fn(_) -> _>(|(n, t)| (n, <Bytes as PartialEq>::eq, t));
    debug!("Starting protocol negotiation");

    let negotiation = match endpoint {
        Endpoint::Listener => future::Either::A(multistream_select::listener_select_proto(connection, iter)),
        Endpoint::Dialer => future::Either::B(multistream_select::dialer_select_proto(connection, iter)),
    };

    negotiation
        .map_err(|err| IoError::new(IoErrorKind::Other, err))
        .then(move |negotiated| {
            match negotiated {
                Ok(_) => debug!("Successfully negotiated protocol upgrade"),
                Err(ref err) => debug!("Error while negotiated protocol upgrade: {:?}", err),
            };
            negotiated
        })
}
