// Copyright 2023 Protocol Labs
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

use crate::{
    multiaddr::{Multiaddr, Protocol},
    transport::{DialOpts, ListenerId, TransportError, TransportEvent},
};
use ip_global::IpExt;
use log::debug;
use std::{
    pin::Pin,
    task::{Context, Poll},
};

/// Dropping all dial requests to non-global IP addresses.
#[derive(Debug, Clone, Default)]
pub struct Transport<T> {
    inner: T,
}

impl<T> Transport<T> {
    pub fn new(transport: T) -> Self {
        Transport { inner: transport }
    }
}

impl<T: crate::Transport + Unpin> crate::Transport for Transport<T> {
    type Output = <T as crate::Transport>::Output;
    type Error = <T as crate::Transport>::Error;
    type ListenerUpgrade = <T as crate::Transport>::ListenerUpgrade;
    type Dial = <T as crate::Transport>::Dial;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        self.inner.listen_on(id, addr)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.inner.remove_listener(id)
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        match addr.iter().next() {
            Some(Protocol::Ip4(a)) => {
                if !IpExt::is_global(&a) {
                    debug!("Not dialing non global IP address {:?}.", a);
                    return Err(TransportError::MultiaddrNotSupported(addr));
                }
                self.inner.dial(addr, opts)
            }
            Some(Protocol::Ip6(a)) => {
                if !IpExt::is_global(&a) {
                    debug!("Not dialing non global IP address {:?}.", a);
                    return Err(TransportError::MultiaddrNotSupported(addr));
                }
                self.inner.dial(addr, opts)
            }
            _ => {
                debug!("Not dialing unsupported Multiaddress {:?}.", addr);
                Err(TransportError::MultiaddrNotSupported(addr))
            }
        }
    }

    fn address_translation(&self, listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.address_translation(listen, observed)
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Pin::new(&mut self.inner).poll(cx)
    }
}
