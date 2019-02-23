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

use super::ffi;
use crate::Addr;
use std::{io, mem, os::raw::{c_int, c_ulong, c_void}};

pub struct HciSocket {
    socket: c_int,
}

impl HciSocket {
    pub fn new() -> Result<HciSocket, io::Error> {
        let socket = unsafe {
            libc::socket(
                libc::AF_BLUETOOTH,
                libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
                ffi::BTPROTO_HCI
            )
        };

        if socket == -1 {
            return Err(io::Error::last_os_error());
        }

        Ok(HciSocket {
            socket,
        })
    }

    pub fn ioctl1<T>(&self, req: c_ulong, param: T) -> Result<(), io::Error> {
        unsafe {
            if libc::ioctl(self.socket, req, param) == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
    }
}

impl Drop for HciSocket {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.socket);
        }
    }
}
