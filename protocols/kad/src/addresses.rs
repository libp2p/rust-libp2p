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

use arrayvec::ArrayVec;
use libp2p_core::Multiaddr;
use std::{fmt, time::Duration, time::Instant};

/// List of addresses of a peer.
#[derive(Clone)]
pub struct Addresses {
    /// Contains an `Instant` when the address expires. If `None`, we are connected to this
    /// address.
    addrs: ArrayVec<[(Multiaddr, Option<Instant>); 6]>,
    /// Time-to-live for addresses we're not connected to.
    expiration: Duration,
}

impl Addresses {
    /// Creates a new list of addresses.
    pub fn new() -> Addresses {
        Self::with_time_to_live(Duration::from_secs(60 * 60))
    }

    /// Creates a new list of addresses. The addresses we're not connected to will use the given
    /// time-to-live before they expire.
    pub fn with_time_to_live(ttl: Duration) -> Addresses {
        Addresses {
            addrs: ArrayVec::new(),
            expiration: ttl,
        }
    }

    /// Returns the list of addresses.
    pub fn iter(&self) -> impl Iterator<Item = &Multiaddr> {
        let now = Instant::now();
        self.addrs.iter().filter_map(move |(addr, exp)| {
            if let Some(exp) = exp {
                if *exp >= now {
                    Some(addr)
                } else {
                    None
                }
            } else {
                Some(addr)
            }
        })
    }

    /// If true, we are connected to all the addresses returned by `iter()`.
    ///
    /// Returns false if the list of addresses is empty.
    pub fn is_connected(&self) -> bool {
        // Note: we're either connected to all addresses or none. There's no in-between.
        self.addrs.first().map(|(_, exp)| exp.is_none()).unwrap_or(false)
    }

    /// If we were connected to that addresses, indicates that we are now disconnected.
    pub fn set_disconnected(&mut self, addr: &Multiaddr) {
        let pos = match self.addrs.iter().position(|(a, _)| a == addr) {
            Some(p) => p,
            None => return,
        };

        // We were already disconnected.
        if self.addrs[pos].1.is_some() {
            return;
        }

        // Address is the only known address.
        if self.addrs.len() == 1 {
            self.addrs[pos].1 = Some(Instant::now() + self.expiration);
            return;
        }

        // We know other connected addresses. Remove this one.
        self.addrs.remove(pos);
    }

    /// Removes the given address from the list. Typically called if an address is determined to
    /// be invalid or unreachable.
    pub fn remove_addr(&mut self, addr: &Multiaddr) {
        if let Some(pos) = self.addrs.iter().position(|(a, _)| a == addr) {
            self.addrs.remove(pos);
        }
    }

    /// Inserts an address in the list. The address is an address we're not connected to, or may
    /// not be connected to.
    pub fn insert_not_connected(&mut self, addr: Multiaddr) {
        // Don't insert if either we're already in the list, or we're connected to any address.
        if self.addrs.iter().any(|(a, expires)| a == &addr || expires.is_none()) {
            return;
        }

        // Do a cleanup pass.
        let now = Instant::now();
        self.addrs.retain(move |(_, exp)| {
            exp.expect("We check above that all the expires are Some") > now
        });

        let _ = self.addrs.try_push((addr, Some(Instant::now() + self.expiration)));
    }

    /// Inserts an address in the list. We know that the address is reachable.
    pub fn insert_connected(&mut self, addr: Multiaddr) {
        if !self.is_connected() {
            self.addrs.clear();
        }

        if self.addrs.iter().all(|(a, _)| *a != addr) {
            let _ = self.addrs.try_push((addr, None));
        }
    }
}

impl Default for Addresses {
    fn default() -> Self {
        Addresses::new()
    }
}

impl fmt::Debug for Addresses {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_list()
            .entries(self.addrs.iter().map(|(a, _)| a))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use libp2p_core::multiaddr;
    use super::Addresses;
    use std::{iter, thread, time::Duration};

    #[test]
    fn insert_connected_after_not_connected() {
        let mut addrs = Addresses::new();
        addrs.insert_not_connected("/ip4/1.2.3.4/tcp/5".parse().unwrap());
        addrs.insert_not_connected("/ip4/6.7.8.9/tcp/5".parse().unwrap());
        addrs.insert_not_connected("/ip4/10.11.12.13/tcp/5".parse().unwrap());
        assert_eq!(addrs.iter().count(), 3);
        assert!(!addrs.is_connected());
        addrs.insert_connected("/ip4/8.9.10.11".parse().unwrap());
        assert_eq!(addrs.iter().count(), 1);
        assert!(addrs.is_connected());
    }

    #[test]
    fn not_connected_expire() {
        let mut addrs = Addresses::with_time_to_live(Duration::from_secs(2));

        addrs.insert_not_connected("/ip4/1.2.3.4/tcp/5".parse().unwrap());
        assert_eq!(addrs.iter().count(), 1);

        thread::sleep(Duration::from_secs(1));
        assert_eq!(addrs.iter().count(), 1);

        addrs.insert_not_connected("/ip4/6.7.8.9/tcp/5".parse().unwrap());
        addrs.insert_not_connected("/ip4/10.11.12.13/tcp/5".parse().unwrap());
        assert_eq!(addrs.iter().count(), 3);

        thread::sleep(Duration::from_secs(1));
        assert_eq!(addrs.iter().count(), 2);

        thread::sleep(Duration::from_secs(1));
        assert_eq!(addrs.iter().count(), 0);
    }

    #[test]
    fn connected_dont_expire() {
        let mut addrs = Addresses::with_time_to_live(Duration::from_secs(1));
        addrs.insert_connected("/ip4/1.2.3.4/tcp/5".parse().unwrap());
        assert_eq!(addrs.iter().count(), 1);

        thread::sleep(Duration::from_secs(2));
        assert_eq!(addrs.iter().count(), 1);
        assert!(addrs.is_connected());
    }

    #[test]
    fn dont_insert_disconnected_if_connected() {
        let mut addrs = Addresses::new();
        addrs.insert_connected("/ip4/1.2.3.4/tcp/5".parse().unwrap());
        assert_eq!(addrs.iter().count(), 1);

        addrs.insert_not_connected("/ip4/5.6.7.8/tcp/5".parse().unwrap());
        assert_eq!(addrs.iter().count(), 1);
        assert!(addrs.is_connected());
    }

    #[test]
    fn disconnect_addr() {
        let mut addrs = Addresses::new();

        addrs.insert_connected("/ip4/1.2.3.4/tcp/5".parse().unwrap());
        addrs.insert_connected("/ip4/6.7.8.9/tcp/5".parse().unwrap());
        assert_eq!(addrs.iter().count(), 2);

        addrs.set_disconnected(&"/ip4/1.2.3.4/tcp/5".parse().unwrap());
        assert_eq!(addrs.iter().count(), 1);
        assert!(addrs.is_connected());

        addrs.set_disconnected(&"/ip4/6.7.8.9/tcp/5".parse().unwrap());
        assert_eq!(addrs.iter().count(), 1);
        assert!(!addrs.is_connected());
    }

    #[test]
    fn max_addrs() {
        // Check that the number of addresses stops increasing even if we continue inserting.
        let mut addrs = Addresses::new();

        let mut previous_loop_count = None;

        for n in 0.. {
            let addr: multiaddr::Multiaddr = iter::once(multiaddr::Protocol::Tcp(n)).collect();
            addrs.insert_not_connected(addr);

            let num = addrs.iter().count();
            if previous_loop_count == Some(num) {
                return; // Test success
            }
            previous_loop_count = Some(num);
        }
    }
}
