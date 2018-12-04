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

extern crate bytes;
extern crate futures;
extern crate libp2p_core;
extern crate void;

use bytes::Bytes;
use futures::future::{self, FutureResult};
use libp2p_core::{Endpoint, Upgrade};
use std::iter;
use void::Void;

#[derive(Debug, Copy, Clone)]
pub struct PlainTextConfig;

impl<C> Upgrade<C> for PlainTextConfig {
    type UpgradeId = ();
    type NamesIter = iter::Once<(Bytes, Self::UpgradeId)>;
    type Output = C;
    type Error = Void;
    type Future = FutureResult<C, Self::Error>;

    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/plaintext/1.0.0"), ()))
    }

    fn upgrade(self, i: C, _: Self::UpgradeId, _: Endpoint) -> Self::Future {
        future::ok(i)
    }
}

