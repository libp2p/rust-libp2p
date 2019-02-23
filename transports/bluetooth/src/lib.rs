// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! Implementation of the libp2p `Transport` trait for Bluetooth.

use futures::{future, prelude::*, try_ready};
use libp2p_core::{Multiaddr, multiaddr::Protocol, Transport, transport::TransportError};
use std::io;

mod addr;
//mod scan;
pub mod sys; // TODO: not pub

pub use self::addr::Addr;

/// Represents the configuration for a Bluetooth transport capability for libp2p.
#[derive(Debug, Clone)]
pub struct BluetoothConfig {
    register_sdp: bool,
}

impl Default for BluetoothConfig {
    fn default() -> BluetoothConfig {
        BluetoothConfig {
            register_sdp: true,
        }
    }
}

impl Transport for BluetoothConfig {
    type Output = sys::BluetoothStream;
    type Error = io::Error;
    type Listener = BluetoothListener;
    type ListenerUpgrade = future::FutureResult<Self::Output, Self::Error>;
    type Dial = future::FutureResult<Self::Output, Self::Error>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), TransportError<Self::Error>> {
        let (mac, port) = multiaddr_to_rfcomm(addr.clone())?;       // TODO: don't clone
        let listener = sys::BluetoothListener::bind(mac, port).map_err(TransportError::Other)?;
        Ok((BluetoothListener { inner: listener }, addr))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let (mac, port) = multiaddr_to_rfcomm(addr)?;
        let socket = sys::BluetoothStream::connect(mac, port).map_err(TransportError::Other)?;
        Ok(future::ok(socket))
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        // TODO: ?
        None
    }
}

pub struct BluetoothListener {
    inner: sys::BluetoothListener,
}

impl Stream for BluetoothListener {
    type Item = (future::FutureResult<sys::BluetoothStream, io::Error>, Multiaddr);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let socket = try_ready!(self.inner.poll());
        Ok(Async::Ready(socket.map(|stream| {
            let addr = "/bluetooth/34:e1:2d:90:20:bc/l2cap/3/rfcomm/10".parse().unwrap();
            (future::ok(stream), addr)
        })))
    }
}

// This type of logic should probably be moved into the multiaddr package
fn multiaddr_to_rfcomm<T>(addr: Multiaddr) -> Result<(Addr, u8), TransportError<T>> {
    // TODO: TransportError::MultiaddrNotSupported(addr)
    let mut iter = addr.iter();
    let proto1 = match iter.next() {
        Some(p) => p,
        None => return Err(TransportError::MultiaddrNotSupported(addr)),
    };

    let proto2 = match iter.next() {
        Some(p) => p,
        None => return Err(TransportError::MultiaddrNotSupported(addr)),
    };

    let proto3 = match iter.next() {
        Some(p) => p,
        None => return Err(TransportError::MultiaddrNotSupported(addr)),
    };

    if iter.next().is_some() {
        return Err(TransportError::MultiaddrNotSupported(addr));
    }

    match (proto1, proto2, proto3) {
        (Protocol::Bluetooth(mac), Protocol::L2cap(3), Protocol::Rfcomm(port)) => {
            if port > 30 {
                return Err(TransportError::MultiaddrNotSupported(addr));
            }

            Ok((Addr::from_big_endian(mac), port))
        },
        _ => Err(TransportError::MultiaddrNotSupported(addr)),
    }
}

#[cfg(test)]
mod tests {
    use crate::BluetoothConfig;
    use futures::prelude::*;
    use libp2p_core::Transport;

    #[test]
    fn connect_to_self() {
        let config = BluetoothConfig::default();

        // TODO: correct addresses; also, doesn't work
        let listener = config.clone().listen_on("/bluetooth/00:00:00:00:00:00/l2cap/3/rfcomm/5".parse().unwrap()).unwrap().0;
        let dialer = config.dial("/bluetooth/FF:FF:FF:00:00:00/l2cap/3/rfcomm/5".parse().unwrap()).unwrap();

        let listener = listener.into_future().map_err(|(err, _)| err).map(|(inc, _)| inc.unwrap().0).map(|_| ());
        let dialer = dialer.map(|_| ());

        let combined = listener.select(dialer).map_err(|_| panic!()).and_then(|(_, next)| next).map(|_| ());
        tokio::runtime::Runtime::new().unwrap().block_on(combined).unwrap();
    }
}
