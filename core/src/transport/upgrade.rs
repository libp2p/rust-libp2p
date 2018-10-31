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
    transport::{self, Dialer, Listener},
    upgrade::{OutboundUpgrade, InboundUpgrade, apply_inbound, apply_outbound}
};
use tokio_io::{AsyncRead, AsyncWrite};

#[derive(Debug, Copy, Clone)]
pub struct DialerUpgrade<D, U> { dialer: D, upgrade: U }

impl<D, U> DialerUpgrade<D, U> {
    pub fn new(dialer: D, upgrade: U) -> Self {
        DialerUpgrade { dialer, upgrade }
    }
}

impl<D, U> Dialer for DialerUpgrade<D, U>
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
    type Error = transport::Error<D::Error, U::Error>;
    type Outbound = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Outbound, (Self, Multiaddr)> {
        let upgrade = self.upgrade;
        match self.dialer.dial(addr.clone()) {
            Ok(outbound) => {
                let future = outbound
                    .map_err(transport::Error::Transport)
                    .and_then(move |x| apply_outbound(x, upgrade).map_err(transport::Error::Upgrade));
                Ok(Box::new(future))
            }
            Err((dialer, addr)) => Err((DialerUpgrade::new(dialer, upgrade), addr))
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct ListenerUpgrade<L, U> { listener: L, upgrade: U }

impl<L, U> ListenerUpgrade<L, U> {
    pub fn new(listener: L, upgrade: U) -> Self {
        ListenerUpgrade { listener, upgrade }
    }
}

impl<L, U> Listener for ListenerUpgrade<L, U>
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
    type Error = transport::Error<L::Error, U::Error>;
    type Inbound = Box<Stream<Item = (Self::Upgrade, Multiaddr), Error = std::io::Error> + Send>;
    type Upgrade = Box<Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Inbound, Multiaddr), (Self, Multiaddr)> {
        let upgrade = self.upgrade;
        match self.listener.listen_on(addr) {
            Ok((inbound, addr)) => {
                let stream = inbound
                    .map(move |(future, addr)| {
                        let upgrade = upgrade.clone();
                        let future = future
                            .map_err(transport::Error::Transport)
                            .and_then(move |x| apply_inbound(x, upgrade).map_err(transport::Error::Upgrade));
                    (Box::new(future) as Box<_>, addr)
                });
                Ok((Box::new(stream), addr))
            }
            Err((listener, addr)) => Err((ListenerUpgrade::new(listener, upgrade), addr)),
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.listener.nat_traversal(server, observed)
    }
}
