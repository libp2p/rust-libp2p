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

use crate::{Multiaddr, transport::{Dialer, Listener}};
use futures::prelude::*;
use std::marker::PhantomData;

#[derive(Debug, Copy, Clone)]
pub struct RefusedDialer<T, E> {
    _output: PhantomData<T>,
    _error: PhantomData<E>
}

impl<T, E> RefusedDialer<T, E> {
    pub fn new() -> Self {
        RefusedDialer {
            _output: PhantomData,
            _error: PhantomData
        }
    }
}

impl<T, E> Dialer for RefusedDialer<T, E> {
    type Output = T;
    type Error = E;
    type Outbound = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
        Err((self, addr))
    }
}

#[derive(Debug, Copy, Clone)]
pub struct RefusedListener<T, E> {
    _output: PhantomData<T>,
    _error: PhantomData<E>
}

impl<T, E> RefusedListener<T, E> {
    pub fn new() -> Self {
        RefusedListener {
            _output: PhantomData,
            _error: PhantomData
        }
    }
}

impl<T, E> Listener for RefusedListener<T, E> {
    type Output = T;
    type Error = E;
    type Inbound = Box<Stream<Item = (Self::Upgrade, Multiaddr), Error = std::io::Error> + Send>;
    type Upgrade = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)> {
        Err((self, addr))
    }

    fn nat_traversal(&self, _server: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        None
    }
}

