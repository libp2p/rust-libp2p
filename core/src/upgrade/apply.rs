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
use futures::prelude::*;
use multiaddr::Multiaddr;
use multistream_select;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use tokio_io::{AsyncRead, AsyncWrite};
use upgrade::{ConnectionUpgrade, Endpoint};

/// Applies a connection upgrade on a socket.
///
/// Returns a `Future` that returns the outcome of the connection upgrade.
#[inline]
pub fn apply<'a, C, U>(
    connection: C,
    upgrade: U,
    endpoint: Endpoint,
    remote_addr: Multiaddr,
) -> Box<Future<Item = (U::Output, Multiaddr), Error = IoError> + 'a>
where
    U: ConnectionUpgrade<C> + 'a,
    U::NamesIter: Clone, // TODO: not elegant
    C: AsyncRead + AsyncWrite + 'a,
{
    let iter = upgrade
        .protocol_names()
        .map::<_, fn(_) -> _>(|(n, t)| (n, <Bytes as PartialEq>::eq, t));
    let remote_addr2 = remote_addr.clone();
    debug!("Starting protocol negotiation");

    let negotiation = match endpoint {
        Endpoint::Listener => multistream_select::listener_select_proto(connection, iter),
        Endpoint::Dialer => multistream_select::dialer_select_proto(connection, iter),
    };

    let future = negotiation
        .map_err(|err| IoError::new(IoErrorKind::Other, err))
        .then(move |negotiated| {
            match negotiated {
                Ok(_) => debug!("Successfully negotiated protocol upgrade with {}", remote_addr2),
                Err(ref err) => debug!("Error while negotiated protocol upgrade: {:?}", err),
            };
            negotiated
        })
        .and_then(move |(upgrade_id, connection)| {
            let fut = upgrade.upgrade(connection, upgrade_id, endpoint, &remote_addr);
            fut.map(move |c| (c, remote_addr))
        })
        .into_future()
        .then(|val| {
            match val {
                Ok(_) => debug!("Successfully applied negotiated protocol"),
                Err(_) => debug!("Failed to apply negotiated protocol"),
            }
            val
        });

    Box::new(future)
}
