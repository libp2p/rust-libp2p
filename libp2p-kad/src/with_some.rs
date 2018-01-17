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

//! Contains the `WithSome` utility struct, which wraps around a `ConnectionUpgrade` and turns
//! its output into a tuple.

use futures::Future;
use libp2p_swarm::{ConnectionUpgrade, Endpoint};
use multiaddr::Multiaddr;
use tokio_io::{AsyncRead, AsyncWrite};

/// Wraps around a `ConnectionUpgrade` and turns its output into a tuple. The second element of
/// the tuple is the second element of `WithSome`.
///
/// > **Note**: This wrapper is necessary because of limitations in the Rust language (namely, the
/// >           fact that closures are not clonable). If this limitations ever disappears, then
/// >           this struct will no longer be necessary.
#[derive(Debug, Clone)]
pub(crate) struct WithSome<U, T>(pub U, pub T);

impl<C, U, T> ConnectionUpgrade<C> for WithSome<U, T>
    where C: AsyncRead + AsyncWrite,
        U: ConnectionUpgrade<C> + 'static,
        U::Future: 'static,
        T: 'static,
{
    type NamesIter = U::NamesIter;
    type UpgradeIdentifier = U::UpgradeIdentifier;
    type Output = (U::Output, T);
    type Future = Box<Future<Item = Self::Output, Error = <U::Future as Future>::Error>>;

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        self.0.protocol_names()
    }

    #[inline]
    fn upgrade(self, connec: C, ident: Self::UpgradeIdentifier, endpoint: Endpoint,
            addr: &Multiaddr) -> Self::Future
    {
        let value = self.1;
        let fut = self.0
            .upgrade(connec, ident, endpoint, addr)
            .map::<_, _>(move |u| (u, value));
        Box::new(fut)
    }
}
