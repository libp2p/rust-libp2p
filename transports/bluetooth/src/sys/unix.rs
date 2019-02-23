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

use crate::Addr;
use futures::{prelude::*, try_ready};
use std::{ffi::CStr, io, mem, os::unix::io::FromRawFd};

mod ffi;
mod hci_scan;
mod sdp;
mod socket;

pub use self::hci_scan::HciScan as Scan;

pub struct BluetoothStream {
    inner: tokio_reactor::PollEvented<socket::BluetoothSocket>,
}

impl BluetoothStream {
    pub fn connect(dest: Addr, port: u8) -> Result<BluetoothStream, io::Error> {
        let socket = socket::BluetoothSocket::new()?;
        socket.connect(dest, port)?;
        Ok(BluetoothStream {
            inner: tokio_reactor::PollEvented::new(socket)
        })
    }
}

pub struct BluetoothListener {
    inner: tokio_reactor::PollEvented<socket::BluetoothSocket>,
    sdp_registration: Option<sdp::SdpRegistration>,
}

impl BluetoothListener {
    pub fn bind(dest: Addr, port: u8) -> Result<BluetoothListener, io::Error> {
        let socket = socket::BluetoothSocket::new()?;
        socket.bind(dest, port)?;

        let sdp_registration = sdp::register(sdp::RegisterConfig {
            uuid: [0x0, 0x0, 0x0, 0xABCD],
            rfcomm_channel: port,
            service_name: CStr::from_bytes_with_nul(b"libp2p\0").expect("Always ends with 0"),
            service_desc: CStr::from_bytes_with_nul(b"libp2p entry point\0").expect("Always ends with 0"),
            service_prov: CStr::from_bytes_with_nul(b"rust-libp2p\0").expect("Always ends with 0"),
        }).ok();

        Ok(BluetoothListener {
            inner: tokio_reactor::PollEvented::new(socket),
            sdp_registration,
        })
    }
}

impl Stream for BluetoothListener {
    type Item = BluetoothStream;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let ready = mio::Ready::readable();
        try_ready!(self.inner.poll_read_ready(ready));

        match self.inner.get_ref().accept() {
            Ok((client, addr)) => Ok(Async::Ready(Some(BluetoothStream {
                inner: tokio_reactor::PollEvented::new(client)
            }))),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.inner.clear_read_ready(ready)?;
                Ok(Async::NotReady)
            }
            Err(e) => Err(e),
        }
    }
}
