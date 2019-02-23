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
use super::ffi;
use std::{io, mem, os::raw::{c_int, c_ulong}, ptr};

pub struct HciSocket {
    socket: c_int,
}

impl HciSocket {
    pub fn new() -> Result<HciSocket, io::Error> {
        let socket = unsafe {
            libc::socket(
                libc::AF_BLUETOOTH,
                libc::SOCK_RAW | libc::SOCK_CLOEXEC,
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

    /// Performs a scan of the nearby devices.
    pub fn inquiry(&self) -> Result<Vec<Addr>, io::Error> {
        unsafe {
            let dev_id = ffi::hci_get_route(ptr::null_mut());
            if dev_id == -1 {
                return Err(io::Error::last_os_error());
            }

            let num_results: u8 = 255;

            let mut buf: Vec<u8> = Vec::with_capacity(mem::size_of::<ffi::hci_inquiry_req>() + mem::size_of::<ffi::inquiry_info>() * num_results as usize);
            buf.set_len(buf.capacity());
            let (req, results) = buf.split_at_mut(mem::size_of::<ffi::hci_inquiry_req>());
            let mut req: *mut ffi::hci_inquiry_req = req.as_mut_ptr() as *mut _;
            let results: *mut ffi::inquiry_info = results.as_mut_ptr() as *mut _;

            (*req).dev_id = dev_id as u16;
            (*req).flags = ffi::IREQ_CACHE_FLUSH as u16;
            (*req).lap = [0x33, 0x8b, 0x9e];
            (*req).length = 8;     // Timeout; the actual timeout is 1.28 times this value, don't ask me why
            (*req).num_rsp = num_results;

            self.ioctl1(ffi::HCIINQUIRY, req)?;

            let mut out = Vec::with_capacity((*req).num_rsp as usize);
            for elem in (0..(*req).num_rsp).map(|n| results.offset(n as isize)) {
                let addr = Addr::from_little_endian((*elem).baddr.b);
                out.push(addr);
            }

            Ok(out)
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
