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
use super::{ffi, hci_socket::HciSocket};
use futures::{prelude::*, sync::oneshot};
use std::{io, mem, ptr};

/// Request to the HCI for the list of nearby Bluetooth devices.
pub struct HciScan {
    inner: HciScanInner,
}

enum HciScanInner {
    Waiting(oneshot::Receiver<Result<Vec<Addr>, io::Error>>),
    Dispatching(Vec<Addr>),
}

impl HciScan {
    /// Initializes a new scan.
    pub fn new() -> Result<HciScan, io::Error> {
        let (tx, rx) = oneshot::channel();
        start_thread(tx);
        Ok(HciScan {
            inner: HciScanInner::Waiting(rx)
        })
    }

    /// Pulls the discovered devices, or `None` if we have finished enumerating.
    ///
    /// Each device will only appear once in the list.
    ///
    /// Just like `Stream::poll()`, must be executed within the context of a task. If `NotReady` is
    /// returned, the current task is registered then notified when something is ready.
    pub fn poll(&mut self) -> Poll<Option<Addr>, io::Error> {
        loop {
            let list = match self.inner {
                HciScanInner::Waiting(ref mut rx) => {
                    match rx.poll().expect("The background thread panicked") {
                        Async::Ready(list) => list?,
                        Async::NotReady => return Ok(Async::NotReady),
                    }
                },
                HciScanInner::Dispatching(ref mut list) => {
                    if list.is_empty() {
                        return Ok(Async::Ready(None));
                    } else {
                        return Ok(Async::Ready(Some(list.remove(0))));
                    }
                },
            };

            self.inner = HciScanInner::Dispatching(list);
        };
    }
}

fn start_thread(sender: oneshot::Sender<Result<Vec<Addr>, io::Error>>) {
    let _ = sender.send(query());
}

fn query() -> Result<Vec<Addr>, io::Error> {
    unsafe {
        let socket = libc::socket(libc::AF_BLUETOOTH, libc::SOCK_RAW | libc::SOCK_CLOEXEC, ffi::BTPROTO_HCI);
        if socket == 0 {
            return Err(io::Error::last_os_error());
        }

        let dev_id = ffi::hci_get_route(ptr::null_mut());
        if dev_id == -1 {
            libc::close(socket);
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

        let ret = libc::ioctl(socket, ffi::HCIINQUIRY, req as usize);
        if ret < 0 {
            libc::close(socket);
            return Err(io::Error::last_os_error());
        }

        let mut out = Vec::with_capacity((*req).num_rsp as usize);
        for elem in (0..(*req).num_rsp).map(|n| results.offset(n as isize)) {
            let addr = Addr::from_little_endian((*elem).baddr.b);
            out.push(addr);
        }

        libc::close(socket);
        Ok(out)
    }
}
