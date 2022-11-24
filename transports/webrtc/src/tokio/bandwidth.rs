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

use std::sync::atomic::{AtomicU64, Ordering};

/// Holds the counters used when logging bandwidth.
///
/// Can be safely shared between threads.
pub(crate) struct Bandwidth {
    inbound: AtomicU64,
    outbound: AtomicU64,
}
impl Bandwidth {
    /// Creates a new [`Bandwidth`].
    pub(crate) fn new() -> Self {
        Self {
            inbound: AtomicU64::new(0),
            outbound: AtomicU64::new(0),
        }
    }

    /// Increases the number of bytes received.
    pub(crate) fn add_inbound(&self, n: u64) {
        self.inbound.fetch_add(n, Ordering::Relaxed);
    }

    /// Increases the number of bytes sent for a given address.
    pub(crate) fn add_outbound(&self, n: u64) {
        self.outbound.fetch_add(n, Ordering::Relaxed);
    }

    /// Gets the number of bytes received & sent. Resets both counters to zero after the call.
    pub(crate) fn reset(&self) -> (u64, u64) {
        (
            self.inbound.swap(0, Ordering::Relaxed),
            self.outbound.swap(0, Ordering::Relaxed),
        )
    }
}
