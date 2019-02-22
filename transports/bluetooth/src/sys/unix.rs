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

use futures::{prelude::*, try_ready};
use std::{io, mem, os::unix::io::FromRawFd};

#[repr(C)]
#[derive(Copy, Debug, Clone)]
struct sockaddr_rc {
    rc_family: libc::sa_family_t,
    rc_bdaddr: [u8; 6],
    rc_channel: u8,
}

pub struct BluetoothStream {
    inner: tokio_uds::UnixStream,
}

impl BluetoothStream {
    pub fn connect(dest: [u8; 6], port: u8) -> Result<BluetoothStream, io::Error> {
        let socket = unsafe {
            let socket = libc::socket(
                libc::AF_BLUETOOTH,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                3 /* BTPROTO_RFCOMM */
            );

            if socket == -1 {
                return Err(io::Error::last_os_error());
            }

            let params = sockaddr_rc {
                rc_family: libc::AF_BLUETOOTH as u16,
                rc_bdaddr: dest,
                rc_channel: port,
            };

            let status = libc::connect(
                socket,
                &params as *const sockaddr_rc as *const _,
                mem::size_of_val(&params) as u32
            );

            if status == -1 {
                let err = io::Error::last_os_error();
                // TODO: handle?
                /*if err.kind() != io::ErrorKind::WouldBlock {
                    libc::close(socket);
                    return Err(io::Error::last_os_error());
                }*/
            }

            std::os::unix::net::UnixStream::from_raw_fd(socket)
        };

        tokio_uds::UnixStream::from_std(socket, &Default::default())
            .map(|inner| {
                BluetoothStream {
                    inner
                }
            })
    }
}

pub struct BluetoothListener {
    inner: tokio_uds::Incoming,
}

impl BluetoothListener {
    pub fn bind(dest: [u8; 6], port: u8) -> Result<BluetoothListener, io::Error> {
        let socket = unsafe {
            let socket = libc::socket(
                libc::AF_BLUETOOTH,
                libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                3 /* BTPROTO_RFCOMM */
            );

            if socket == -1 {
                return Err(io::Error::last_os_error());
            }

            let params = sockaddr_rc {
                rc_family: libc::AF_BLUETOOTH as u16,
                rc_bdaddr: dest,
                rc_channel: port,
            };

            let status = libc::bind(
                socket,
                &params as *const sockaddr_rc as *const _,
                mem::size_of_val(&params) as u32
            );

            if status == -1 {
                libc::close(socket);
                return Err(io::Error::last_os_error());
            }

            let status = libc::listen(socket, 8);       // TODO: allow configuring this 8
            if status == -1 {
                libc::close(socket);
                return Err(io::Error::last_os_error());
            }

            std::os::unix::net::UnixListener::from_raw_fd(socket)
        };

        tokio_uds::UnixListener::from_std(socket, &Default::default())
            .map(|inner| {
                BluetoothListener {
                    inner: inner.incoming()
                }
            })
    }
}

impl Stream for BluetoothListener {
    type Item = BluetoothStream;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let socket = try_ready!(self.inner.poll());
        Ok(Async::Ready(socket.map(|i| BluetoothStream { inner: i })))
    }
}
