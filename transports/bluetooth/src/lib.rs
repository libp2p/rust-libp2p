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

use bluetooth_serial_port::BtAddr;
use futures::{future, prelude::*};
use libp2p_core::{Multiaddr, Transport, transport::TransportError};
use std::io;

// TODO: yeah, no
pub use bluetooth_serial_port::scan_devices;

/// Represents the configuration for a Bluetooth transport capability for libp2p.
#[derive(Debug, Clone, Default)]
pub struct BluetoothConfig {
}

impl BluetoothConfig {
}

impl Transport for BluetoothConfig {
    type Output = bluetooth_serial_port::BtSocket;
    type Error = io::Error;
    type Listener = futures::stream::Empty<(Self::ListenerUpgrade, Multiaddr), Self::Error>;
    type ListenerUpgrade = futures::future::FutureResult<Self::Output, Self::Error>;
    type Dial = futures::future::FutureResult<Self::Output, Self::Error>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let addr = multiaddr_to_mac(addr)?;
        let mut socket = bluetooth_serial_port::BtSocket::new(bluetooth_serial_port::BtProtocol::RFCOMM).unwrap();
        socket.connect(addr).unwrap();
        Ok(future::ok(socket))
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        None
    }
}

// This type of logic should probably be moved into the multiaddr package
fn multiaddr_to_mac<T>(addr: Multiaddr) -> Result<BtAddr, TransportError<T>> {
    // TODO: TransportError::MultiaddrNotSupported(addr)
    /*let mut iter = addr.iter();
    let proto1 = iter.next().ok_or(())?;
    let proto2 = iter.next().ok_or(())?;

    if iter.next().is_some() {
        return Err(());
    }

    match (proto1, proto2) {
        (Protocol::Ip4(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        (Protocol::Ip6(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        _ => Err(()),
    }*/
    // TODO:
    Ok(BtAddr([0xe8, 0x07, 0xbf, 0x3f, 0x03, 0xa6]))
}
