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

use std::io::Error as IoError;
use futures::{future, prelude::*};
use tokio_io::{AsyncRead, AsyncWrite};
use upgrade::{ConnectionUpgrade, Endpoint};
use Multiaddr;

/// Applies a closure on the output of a connection upgrade.
#[inline]
pub fn map_with_addr<U, F, I, O>(upgrade: U, map: F) -> MapAddr<U, F>
    where F: FnOnce(I, &Multiaddr) -> O
{
    MapAddr { upgrade, map }
}

/// Application of a closure on the output of a connection upgrade.
#[derive(Debug, Copy, Clone)]
pub struct MapAddr<U, F> {
    upgrade: U,
    map: F,
}

impl<C, U, F, O, Maf> ConnectionUpgrade<C, Maf> for MapAddr<U, F>
where
    U: ConnectionUpgrade<C, Maf>,
    U::Future: Send + 'static,     // TODO: 'static :(
    U::MultiaddrFuture: Future<Item = Multiaddr, Error = IoError> + Send + 'static,    // TODO: 'static :(
    U::Output: Send + 'static,     // TODO: 'static :(
    C: AsyncRead + AsyncWrite,
    F: FnOnce(U::Output, &Multiaddr) -> O + Send + 'static,     // TODO: 'static :(
{
    type NamesIter = U::NamesIter;
    type UpgradeIdentifier = U::UpgradeIdentifier;

    fn protocol_names(&self) -> Self::NamesIter {
        self.upgrade.protocol_names()
    }

    type Output = O;
    type MultiaddrFuture = future::FutureResult<Multiaddr, IoError>;
    type Future = Box<Future<Item = (O, Self::MultiaddrFuture), Error = IoError> + Send>;

    fn upgrade(
        self,
        socket: C,
        id: Self::UpgradeIdentifier,
        ty: Endpoint,
        remote_addr: Maf,
    ) -> Self::Future {
        let map = self.map;
        let fut = self.upgrade
            .upgrade(socket, id, ty, remote_addr)
            .and_then(|(out, addr)| {
                addr.map(move |addr| {
                    let out = map(out, &addr);
                    (out, future::ok(addr))
                })
            });
        Box::new(fut) as Box<_>
    }
}
