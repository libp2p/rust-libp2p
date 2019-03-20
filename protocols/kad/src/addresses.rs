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

use libp2p_core::Multiaddr;
use smallvec::SmallVec;
use std::fmt;

/// List of addresses of a peer.
#[derive(Clone)]
pub struct Addresses {
    addrs: SmallVec<[Multiaddr; 6]>,
}

impl Addresses {
    /// Creates a new list of addresses.
    pub fn new() -> Addresses {
        Addresses {
            addrs: SmallVec::new(),
        }
    }

    /// Returns the list of addresses.
    pub fn iter(&self) -> impl Iterator<Item = &Multiaddr> {
        self.addrs.iter()
    }

    /// Returns true if the list of addresses is empty.
    pub fn is_empty(&self) -> bool {
        self.addrs.is_empty()
    }

    /// Removes the given address from the list. Typically called if an address is determined to
    /// be invalid or unreachable.
    pub fn remove(&mut self, addr: &Multiaddr) {
        if let Some(pos) = self.addrs.iter().position(|a| a == addr) {
            self.addrs.remove(pos);
        }

        if self.addrs.len() <= self.addrs.inline_size() {
            self.addrs.shrink_to_fit();
        }
    }

    /// Clears the list. It is empty afterwards.
    pub fn clear(&mut self) {
        self.addrs.clear();
        self.addrs.shrink_to_fit();
    }

    /// Inserts an address in the list. No effect if the address was already in the list.
    pub fn insert(&mut self, addr: Multiaddr) {
        if self.addrs.iter().all(|a| *a != addr) {
            self.addrs.push(addr);
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
            .entries(self.addrs.iter())
            .finish()
    }
}
