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
use std::sync::Mutex;

/// Holds the counters used when logging bandwidth.
///
/// Can be safely shared between threads.
pub(crate) struct Bandwidth {
    conns: Mutex<FnvHashMap<SocketAddr, ConnBandwidth>>,
}

pub(crate) struct ConnBandwidth {
    pub inbound: u64,
    pub outbound: u64,
}

impl Bandwidth {
    /// Creates a new [`Bandwidth`].
    pub(crate) fn new() -> Self {
        Self {
            conns: Mutex::new(FnvHashMap::default()),
        }
    }

    /// Increases the number of bytes received for a given address.
    pub(crate) fn add_inbound(&self, addr: &SocketAddr, n: u64) {
        let mut lock = self.conns.lock().unwrap();
        let conn_bandwidth = lock.entry(*addr).or_insert(ConnBandwidth {
            inbound: 0,
            outbound: 0,
        });
        conn_bandwidth.inbound += n;
    }

    /// Increases the number of bytes sent for a given address.
    pub(crate) fn add_outbound(&self, addr: &SocketAddr, n: u64) {
        let mut lock = self.conns.lock().unwrap();
        let conn_bandwidth = lock.entry(*addr).or_insert(ConnBandwidth {
            inbound: 0,
            outbound: 0,
        });
        conn_bandwidth.outbound += n;
    }

    /// Resets measurements for a given address and returns the removed values.
    pub(crate) fn reset(&self, addr: &SocketAddr) -> (u64, u64) {
        if let Some(conn_bandwidth) = self.conns.lock().unwrap().remove(addr) {
            (conn_bandwidth.inbound, conn_bandwidth.outbound)
        } else {
            (0, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn bandwidth() {
        let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8081);

        let b = Bandwidth::new();

        assert_eq!((0, 0), b.reset(&addr1));
        assert_eq!((0, 0), b.reset(&addr2));

        b.add_inbound(&addr1, 10);
        b.add_outbound(&addr2, 5);

        assert_eq!((10, 0), b.reset(&addr1));
        assert_eq!((0, 5), b.reset(&addr2));
    }
}
