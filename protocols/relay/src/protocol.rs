// Copyright 2021 Protocol Labs.
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

use std::time::Duration;

use libp2p_swarm::StreamProtocol;

use crate::proto;

pub(crate) mod inbound_hop;
pub(crate) mod inbound_stop;
pub(crate) mod outbound_hop;
pub(crate) mod outbound_stop;
pub const HOP_PROTOCOL_NAME: StreamProtocol =
    StreamProtocol::new("/libp2p/circuit/relay/0.2.0/hop");
pub const STOP_PROTOCOL_NAME: StreamProtocol =
    StreamProtocol::new("/libp2p/circuit/relay/0.2.0/stop");

pub(crate) const MAX_MESSAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit {
    duration: Option<Duration>,
    data_in_bytes: Option<u64>,
}

impl Limit {
    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    pub fn data_in_bytes(&self) -> Option<u64> {
        self.data_in_bytes
    }
}

impl From<proto::Limit> for Limit {
    fn from(limit: proto::Limit) -> Self {
        Limit {
            duration: limit.duration.map(|d| Duration::from_secs(d.into())),
            data_in_bytes: limit.data,
        }
    }
}
