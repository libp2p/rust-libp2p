// Copyright 2017 Parity Technologies (UK) Ltd.
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
use std::{io, iter};
use tokio_io::{AsyncRead, AsyncWrite};
use upgrade::{ConnectionUpgrade, Endpoint};

/// Implementation of `ConnectionUpgrade` that always fails to negotiate.
#[derive(Debug, Copy, Clone)]
pub struct DeniedConnectionUpgrade;

impl<C, Maf> ConnectionUpgrade<C, Maf> for DeniedConnectionUpgrade
where
    C: AsyncRead + AsyncWrite,
{
    type NamesIter = iter::Empty<(Bytes, ())>;
    type UpgradeIdentifier = (); // TODO: could use `!`
    type Output = (); // TODO: could use `!`
    type MultiaddrFuture = Box<Future<Item = Multiaddr, Error = io::Error> + Send + Sync>; // TODO: could use `!`
    type Future = Box<Future<Item = ((), Self::MultiaddrFuture), Error = io::Error> + Send + Sync>; // TODO: could use `!`

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::empty()
    }

    #[inline]
    fn upgrade(self, _: C, _: Self::UpgradeIdentifier, _: Endpoint, _: Maf) -> Self::Future {
        unreachable!("the denied connection upgrade always fails to negotiate")
    }
}
