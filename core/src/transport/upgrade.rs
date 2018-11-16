// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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
    transport::Transport,
    upgrade::{OutboundUpgrade, InboundUpgrade, UpgradeInfo, apply_inbound, apply_outbound}
};
use tokio_io::{AsyncRead, AsyncWrite};

#[derive(Debug, Copy, Clone)]
pub struct Upgrade<T, U> { inner: T, upgrade: U }

impl<T, U> Upgrade<T, U> {
    pub fn new(inner: T, upgrade: U) -> Self {
        Upgrade { inner, upgrade }
    }
}

impl<D, U, O, E> Transport for Upgrade<D, U>
where
    D: Transport,
    D::Dial: Send + 'static,
    D::Listener: Send + 'static,
    D::ListenerUpgrade: Send + 'static,
    D::Output: AsyncRead + AsyncWrite + Send + 'static,
    U: InboundUpgrade<D::Output, Output = O, Error = E>,
    U: OutboundUpgrade<D::Output, Output = O, Error = E> + Send + Clone + 'static,
    <U as UpgradeInfo>::NamesIter: Clone + Send,
    <U as UpgradeInfo>::UpgradeId: Send,
    <U as InboundUpgrade<D::Output>>::Future: Send,
    <U as OutboundUpgrade<D::Output>>::Future: Send,
    E: std::error::Error + Send + Sync + 'static
{
    type Output = O;
    type Listener = Box<Stream<Item = (Self::ListenerUpgrade, Multiaddr), Error = std::io::Error> + Send>;
    type ListenerUpgrade = Box<Future<Item = Self::Output, Error = std::io::Error> + Send>;
    type Dial = Box<Future<Item = Self::Output, Error = std::io::Error> + Send>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        let upgrade = self.upgrade;
        match self.inner.dial(addr.clone()) {
            Ok(outbound) => {
                let future = outbound
                    .and_then(move |x| apply_outbound(x, upgrade).map_err(|e| {
                        std::io::Error::new(std::io::ErrorKind::Other, e)
                    }));
                Ok(Box::new(future))
            }
            Err((dialer, addr)) => Err((Upgrade::new(dialer, upgrade), addr))
        }
    }

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let upgrade = self.upgrade;
        match self.inner.listen_on(addr) {
            Ok((inbound, addr)) => {
                let stream = inbound
                    .map(move |(future, addr)| {
                        let upgrade = upgrade.clone();
                        let future = future
                            .and_then(move |x| apply_inbound(x, upgrade).map_err(|e| {
                                 std::io::Error::new(std::io::ErrorKind::Other, e)
                            }));
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
