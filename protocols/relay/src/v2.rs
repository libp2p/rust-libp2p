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

//! Implementation of the [libp2p circuit relay v2
//! specification](https://github.com/libp2p/specs/blob/master/relay/circuit-v2.md).

pub mod client;
mod copy_future;
pub mod relay;

pub mod protocol {
    #[deprecated(
        since = "0.15.0",
        note = "Use libp2p_relay::InboundHopFatalUpgradeError instead."
    )]
    pub type InboundHopFatalUpgradeError = crate::protocol::inbound_hop::FatalUpgradeError;

    #[deprecated(
        since = "0.15.0",
        note = "Use libp2p_relay::InboundStopFatalUpgradeError instead."
    )]
    pub type InboundStopFatalUpgradeError = crate::protocol::inbound_stop::FatalUpgradeError;

    #[deprecated(
        since = "0.15.0",
        note = "Use libp2p_relay::OutboundHopFatalUpgradeError instead."
    )]
    pub type OutboundHopFatalUpgradeError = crate::protocol::outbound_hop::FatalUpgradeError;

    #[deprecated(
        since = "0.15.0",
        note = "Use libp2p_relay::OutboundStopFatalUpgradeError instead."
    )]
    pub type OutboundStopFatalUpgradeError = crate::protocol::outbound_stop::FatalUpgradeError;

    #[deprecated(
        since = "0.15.0",
        note = "Use libp2p_relay::HOP_PROTOCOL_NAME instead."
    )]
    pub const HOP_PROTOCOL_NAME: &[u8; 31] = crate::HOP_PROTOCOL_NAME;

    #[deprecated(
        since = "0.15.0",
        note = "Use libp2p_relay::STOP_PROTOCOL_NAME instead."
    )]
    pub const STOP_PROTOCOL_NAME: &[u8; 32] = crate::STOP_PROTOCOL_NAME;
}

#[deprecated(since = "0.15.0", note = "Use libp2p_relay::RequestId instead.")]
pub type RequestId = super::RequestId;
