// Copyright 2022 Parity Technologies (UK) Ltd.
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

use fnv::FnvHashMap;

use std::net::SocketAddr;
use std::sync::RwLock;

/// Holds the counters used when logging bandwidth.
///
/// Can be safely shared between threads.
pub(crate) struct Bandwidth {
    conns: RwLock<FnvHashMap<SocketAddr, ConnBandwidth>>,
}

struct ConnBandwidth {
    inbound: u64,
    outbound: u64,
}

impl Bandwidth {
    /// Creates a new [`Bandwidth`].
    pub(crate) fn new() -> Self {
        Self {
            conns: RwLock::new(FnvHashMap::default()),
        }
    }

    /// Increases the number of bytes received for a given address.
    pub(crate) fn add_inbound(&self, addr: SocketAddr, n: u64) {
        let mut lock = self.conns.write().unwrap();
        let conn_bandwidth = lock.entry(addr).or_insert(ConnBandwidth {
            inbound: n,
            outbound: 0,
        });
        conn_bandwidth.inbound += n;
    }

    /// Increases the number of bytes sent for a given address.
    pub(crate) fn add_outbound(&self, addr: SocketAddr, n: u64) {
        let mut lock = self.conns.write().unwrap();
        let conn_bandwidth = lock.entry(addr).or_insert(ConnBandwidth {
            inbound: 0,
            outbound: n,
        });
        conn_bandwidth.outbound += n;
    }

    /// Gets the number of bytes received for a given address.
    ///
    /// > **Note**: This method is by design subject to race conditions. The returned value should
    /// >           only ever be used for statistics purposes.
    pub(crate) fn inbound(&self, addr: &SocketAddr) -> u64 {
        if let Some(conn_bandwidth) = self.conns.read().unwrap().get(addr) {
            conn_bandwidth.inbound
        } else {
            0
        }
    }

    /// Gets the number of bytes sent for a given address.
    ///
    /// > **Note**: This method is by design subject to race conditions. The returned value should
    /// >           only ever be used for statistics purposes.
    pub(crate) fn outbound(&self, addr: &SocketAddr) -> u64 {
        if let Some(conn_bandwidth) = self.conns.read().unwrap().get(addr) {
            conn_bandwidth.outbound
        } else {
            0
        }
    }
}
