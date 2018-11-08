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

use futures::prelude::*;
use multiaddr::Multiaddr;
use crate::{
    transport::{Dialer, Listener, Error},
    upgrade::{OutboundUpgrade, InboundUpgrade, apply_inbound, apply_outbound}
};
use tokio_io::{AsyncRead, AsyncWrite};

#[derive(Debug, Copy, Clone)]
pub struct Upgrade<T, U> { inner: T, upgrade: U }

impl<T, U> Upgrade<T, U> {
    pub fn new(inner: T, upgrade: U) -> Self {
        Upgrade { inner, upgrade }
    }
}

impl<D, U> Dialer for Upgrade<D, U>
where
    D: Dialer + 'static,
    D::Outbound: Send,
    D::Output: AsyncRead + AsyncWrite + Send,
    U: OutboundUpgrade<D::Output> + Send + 'static,
    U::NamesIter: Send,
    U::UpgradeId: Send,
    U::Future: Send
{
    type Output = U::Output;
    type Error = Error<D::Error, U::Error>;
    type Outbound = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
        let upgrade = self.upgrade;
        match self.inner.dial(addr.clone()) {
            Ok(outbound) => {
                let future = outbound
                    .map_err(Error::Transport)
                    .and_then(move |x| apply_outbound(x, upgrade).map_err(Error::Upgrade));
                Ok(Box::new(future))
            }
            Err((dialer, addr)) => Err((Upgrade::new(dialer, upgrade), addr))
        }
    }
}

impl<L, U> Listener for Upgrade<L, U>
where
    L: Listener + 'static,
    L::Inbound: Send,
    L::Upgrade: Send,
    L::Output: AsyncRead + AsyncWrite + Send,
    U: InboundUpgrade<L::Output> + Clone + Send + 'static,
    U::NamesIter: Send + Clone,
    U::UpgradeId: Send,
    U::Future: Send
{
    type Output = U::Output;
    type Error = Error<L::Error, U::Error>;
    type Inbound = Box<Stream<Item = (Self::Upgrade, Multiaddr), Error = std::io::Error> + Send>;
    type Upgrade = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)> {
        let upgrade = self.upgrade;
        match self.inner.listen_on(addr) {
            Ok((inbound, addr)) => {
                let stream = inbound
                    .map(move |(future, addr)| {
                        let upgrade = upgrade.clone();
                        let future = future
                            .map_err(Error::Transport)
                            .and_then(move |x| apply_inbound(x, upgrade).map_err(Error::Upgrade));
                    (Box::new(future) as Box<_>, addr)
                });
                Ok((Box::new(stream), addr))
            }
            Err((listener, addr)) => Err((Upgrade::new(listener, upgrade), addr)),
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.nat_traversal(server, observed)
    }
}
