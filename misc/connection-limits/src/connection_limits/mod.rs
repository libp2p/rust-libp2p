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

mod memory;
pub use memory::{MemoryUsageBasedConnectionLimits, MemoryUsageLimitExceeded};

mod r#static;
pub use r#static::{StaticConnectionLimits, StaticLimitExceeded};

use libp2p_swarm::ConnectionDenied;
use std::fmt;

pub trait ConnectionLimitsChecker {
    fn check_limit(&self, kind: ConnectionKind, current: usize) -> Result<(), ConnectionDenied>;
}

#[derive(Debug, Clone, Copy)]
pub enum ConnectionKind {
    PendingIncoming,
    PendingOutgoing,
    EstablishedIncoming,
    EstablishedOutgoing,
    EstablishedPerPeer,
    EstablishedTotal,
}

impl fmt::Display for ConnectionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ConnectionKind::*;

        match self {
            PendingIncoming => write!(f, "pending incoming connections"),
            PendingOutgoing => write!(f, "pending outgoing connections"),
            EstablishedIncoming => write!(f, "established incoming connections"),
            EstablishedOutgoing => write!(f, "established outgoing connections"),
            EstablishedPerPeer => write!(f, "established connections per peer"),
            EstablishedTotal => write!(f, "established connections"),
        }
    }
}
