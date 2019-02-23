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
use std::io;

#[cfg(unix)]
#[path = "sys/unix.rs"]
pub mod platform;       // TODO: not pub

pub struct BluetoothStream {
    inner: platform::BluetoothStream,
}

impl BluetoothStream {
    pub fn connect(addr: Addr, port: u8) -> BluetoothStreamFuture {
        BluetoothStreamFuture {
            inner: platform::BluetoothStream::connect(addr, port)
        }
    }
}

pub struct BluetoothStreamFuture {
    inner: platform::BluetoothStreamFuture,
}

impl Future for BluetoothStreamFuture {
    type Item = BluetoothStream;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let socket = try_ready!(self.inner.poll());
        Ok(Async::Ready(BluetoothStream {
            inner: socket,
        }))
    }
}

impl io::Read for BluetoothStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.inner.read(buf)
    }
}

impl tokio_io::AsyncRead for BluetoothStream {
    // TODO: specialize functions
}

impl io::Write for BluetoothStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        self.inner.flush()
    }
}

impl tokio_io::AsyncWrite for BluetoothStream {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.inner.shutdown()
    }
}

pub struct BluetoothListener {
    inner: platform::BluetoothListener,
}

impl BluetoothListener {
    pub fn bind(addr: Addr, port: u8) -> io::Result<(BluetoothListener, u8)> {
        crate::discoverable::enable_discoverable(&addr)?;

        let (inner, actual_port) = if port != 0 {
            (platform::BluetoothListener::bind(addr, port)?, port)
        } else {
            (1..30)
                .filter_map(|port| {
                    platform::BluetoothListener::bind(addr, port).ok().map(|s| (s, port))
                })
                .next()
                .ok_or_else(|| io::Error::last_os_error())?
        };

        let inner = BluetoothListener {
            inner
        };

        Ok((inner, actual_port))
    }
}

impl Stream for BluetoothListener {
    type Item = (BluetoothStream, Addr);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let socket = try_ready!(self.inner.poll());
        Ok(Async::Ready(socket.map(|(i, a)| (BluetoothStream { inner: i }, a))))
    }
}

/// Service that scans nearby Bluetooth devices.
pub struct Scan {
    inner: platform::Scan,
}

impl Scan {
    /// Initializes a new scan.
    // TODO: change API to have scan() instead
    pub fn new() -> Result<Scan, io::Error> {
        Ok(Scan {
            inner: platform::Scan::new()?,
        })
    }

    // TODO: wrong doc
    /// Pulls the latest discovered devices.
    ///
    /// Note that this method doesn't cache the list of devices. The same device will be returned
    /// regularly.
    ///
    /// Just like `Future::poll()`, must be executed within the context of a task. If `NotReady` is
    /// returned, the current task is registered then notified when something is ready.
    pub fn poll(&mut self) -> Poll<Option<Addr>, io::Error> {
        self.inner.poll()
    }
}

// TODO: test that things don't panic or crash or whatever
