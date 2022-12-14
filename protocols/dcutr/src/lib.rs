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

//! Implementation of the [libp2p Direct Connection Upgrade through Relay
//! specification](https://github.com/libp2p/specs/blob/master/relay/DCUtR.md).

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod behaviour_impl; // TODO: Rename back `behaviour` once deprecation symbols are removed.
mod handler;
mod protocol;
#[allow(clippy::derive_partial_eq_without_eq)]
mod message_proto {
    include!(concat!(env!("OUT_DIR"), "/holepunch.pb.rs"));
}

pub use behaviour_impl::Behaviour;
pub use behaviour_impl::Error;
pub use behaviour_impl::Event;
pub use protocol::PROTOCOL_NAME;
pub mod inbound {
    pub use crate::protocol::inbound::UpgradeError;
}
pub mod outbound {
    pub use crate::protocol::outbound::UpgradeError;
}

#[deprecated(
    since = "0.9.0",
    note = "Use `libp2p_dcutr::inbound::UpgradeError` instead.`"
)]
pub type InboundUpgradeError = inbound::UpgradeError;

#[deprecated(
    since = "0.9.0",
    note = "Use `libp2p_dcutr::outbound::UpgradeError` instead.`"
)]
pub type OutboundUpgradeError = outbound::UpgradeError;
pub mod behaviour {
    #[deprecated(since = "0.9.0", note = "Use `libp2p_dcutr::Behaviour` instead.`")]
    pub type Behaviour = crate::Behaviour;

    #[deprecated(since = "0.9.0", note = "Use `libp2p_dcutr::Event` instead.`")]
    pub type Event = crate::Event;

    #[deprecated(since = "0.9.0", note = "Use `libp2p_dcutr::Error` instead.`")]
    pub type UpgradeError = crate::Error;
}
