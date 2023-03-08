// Copyright 2023 Protocol Labs.
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

mod behaviour;
mod handler;

use instant::Instant;
use std::sync::atomic::{AtomicUsize, Ordering};

pub use behaviour::{Behaviour, Event};

/// Parameters for a single run, i.e. one stream, sending and receiving data.
#[derive(Debug, Clone, Copy)]
pub struct RunParams {
    pub to_send: usize,
    pub to_receive: usize,
}

/// Timers for a single run, i.e. one stream, sending and receiving data.
#[derive(Debug, Clone, Copy)]
pub struct RunTimers {
    pub write_start: Instant,
    pub write_done: Instant,
    pub read_done: Instant,
}

/// Statistics for a single run, i.e. one stream, sending and receiving data.
#[derive(Debug)]
pub struct RunStats {
    pub params: RunParams,
    pub timers: RunTimers,
}

static NEXT_RUN_ID: AtomicUsize = AtomicUsize::new(1);

/// Connection identifier.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct RunId(usize);

impl RunId {
    /// Returns the next available [`RunId`].
    pub(crate) fn next() -> Self {
        Self(NEXT_RUN_ID.fetch_add(1, Ordering::SeqCst))
    }
}
